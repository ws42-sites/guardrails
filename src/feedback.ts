import { appendFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { z } from 'zod';
import type { Finding } from './types.js';
import type { GuardrailsConfig } from './config.js';

const failureSchema = z.object({ rule: z.string().min(1), file: z.string(), agent: z.string(), message: z.string(), at: z.string().datetime({ offset: true }) }).strict();
export type Failure = z.infer<typeof failureSchema>;
export interface Cluster { rule: string; message: string; entries: Failure[] }
export const draftingPrompt = 'Draft a candidate lint rule from repeated blocking failures. Return a restrictedStrings or ESLint no-restricted-syntax proposal with a one-line rationale for human approval; never apply it.';
export function tagFailures(report: { findings: Finding[] }, agent = process.env.GUARDRAILS_AGENT_NAME ?? 'unknown', now: () => Date = () => new Date()): Failure[] {
  return report.findings.filter((f) => f.mode === 'block' && !f.known).map((f) => ({ rule: f.rule, file: f.file, agent, message: f.message, at: now().toISOString() }));
}
export function appendFailures(cwd: string, config: GuardrailsConfig, report: { findings: Finding[] }, agent?: string): void {
  if (!config.feedback.enabled) return;
  const cliAgent = parseArgs({ strict: false, allowPositionals: true, options: { 'agent-name': { type: 'string' } } }).values['agent-name'];
  const entries = tagFailures(report, agent ?? (typeof cliAgent === 'string' ? cliAgent : undefined));
  if (!entries.length) return;
  const file = resolve(cwd, config.feedback.logFile);
  mkdirSync(dirname(file), { recursive: true });
  appendFileSync(file, entries.map((e) => JSON.stringify(e)).join('\n') + '\n');
}
export function readFailures(file: string): Failure[] {
  return existsSync(file) ? readFileSync(file, 'utf8').split(/\r?\n/).filter((line) => line.trim()).map((line) => failureSchema.parse(JSON.parse(line))) : [];
}
export function clusterFailures(entries: Failure[], minRepeats: number): Cluster[] {
  const groups = new Map<string, Cluster>();
  for (const entry of entries) {
    const message = entry.message.replace(/"[^"\r\n]*"|'[^'\r\n]*'|`[^`\r\n]*`/g, '<quoted>').replace(/(?:[A-Za-z]:)?(?:[\w.-]*[/\\])+[\w./\\:-]+/g, '<path>').replace(/\d+/g, '<number>');
    const key = JSON.stringify([entry.rule, message]);
    const group = groups.get(key) ?? { rule: entry.rule, message, entries: [] };
    group.entries.push(entry);
    groups.set(key, group);
  }
  return [...groups.values()].filter((group) => group.entries.length >= minRepeats);
}
export function draftRule(cluster: Cluster): { kind: 'restrictedStrings' | 'eslint'; snippet: string; rationale: string } {
  const rationale = `${cluster.rule}: ${cluster.entries.length} repeated failures require human review.`;
  const literal = /["'`]([^"'`\r\n]+)["'`]/.exec(cluster.entries[0].message)?.[1];
  return literal
    ? { kind: 'restrictedStrings', snippet: JSON.stringify({ pattern: literal, message: rationale }), rationale }
    : { kind: 'eslint', snippet: JSON.stringify({ 'no-restricted-syntax': ['error', { selector: 'REPLACE_WITH_REVIEWED_SELECTOR', message: rationale }] }), rationale };
}
export function feedbackDraft(file: string, minRepeats: number): { candidates: ReturnType<typeof draftRule>[]; markdown: string } {
  const candidates = clusterFailures(readFailures(file), minRepeats).map(draftRule);
  return { candidates, markdown: '# Guardrails feedback\n\n' + draftingPrompt + '\n\n' + candidates.map((c) => `- ${c.kind}: ${c.rationale}\n\n\`\`\`json\n${c.snippet}\n\`\`\``).join('\n') + '\n' };
}
