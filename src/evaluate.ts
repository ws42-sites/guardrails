import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { z } from 'zod';
import type { GuardrailsConfig } from './config.js';
import type { Mode } from './types.js';

const id = z.number().int().positive();
const at = z.string().datetime({ offset: true });
const common = { pr: id, at, by: z.string().min(1).optional() };
export const ledgerSchema = z.discriminatedUnion('kind', [
  z.object({ ...common, kind: z.literal('escaped-bug'), issue: id, rules: z.array(z.string().min(1)) }).strict(),
  z.object({ ...common, kind: z.literal('false-escalation'), rule: z.string().min(1), label: z.string().min(1), reason: z.string().optional() }).strict(),
]);
export type LedgerEntry = z.infer<typeof ledgerSchema>;
export function readLedger(file: string): LedgerEntry[] {
  return existsSync(file) ? readFileSync(file, 'utf8').split(/\r?\n/).filter((line) => line.trim()).map((line) => ledgerSchema.parse(JSON.parse(line))) : [];
}
const identity = (e: LedgerEntry) => JSON.stringify([e.kind, e.pr, e.kind === 'escaped-bug' ? e.issue : e.rule]);
export function ledgerAdd(file: string, input: unknown): boolean {
  const entry = ledgerSchema.parse(input);
  if (readLedger(file).some((old) => identity(old) === identity(entry))) return false;
  mkdirSync(dirname(file), { recursive: true });
  const separator = existsSync(file) && readFileSync(file, 'utf8').length > 0 && !readFileSync(file, 'utf8').endsWith('\n') ? '\n' : '';
  appendFileSync(file, separator + JSON.stringify(entry) + '\n');
  return true;
}
const label = z.union([z.string(), z.object({ name: z.string() })]).transform((v) => typeof v === 'string' ? v : v.name);
const exportSchema = z.array(z.object({
  number: id,
  mergedAt: at.nullable(),
  linkedIssues: z.array(z.object({ number: id, labels: z.array(label), at, rules: z.array(z.string().min(1)).default([]) })).default([]),
  events: z.array(z.object({ event: z.enum(['blocked', 'labeled']), at, rule: z.string().min(1), label: z.string().optional(), by: z.string().optional() })).default([]),
}));
export function ledgerImport(prs: unknown, config: GuardrailsConfig): LedgerEntry[] {
  const entries: LedgerEntry[] = [];
  for (const pr of exportSchema.parse(prs)) {
    if (pr.mergedAt) for (const issue of pr.linkedIssues) {
      if (issue.labels.includes(config.evaluation.escapedLabel) && Date.parse(issue.at) >= Date.parse(pr.mergedAt))
        entries.push({ kind: 'escaped-bug', pr: pr.number, issue: issue.number, rules: issue.rules, at: issue.at });
    }
    for (const event of pr.events) {
      if (event.event === 'labeled' && event.label?.startsWith('guardrails:') && event.label.endsWith('-approved') &&
          pr.events.some((block) => block.event === 'blocked' && block.rule === event.rule && Date.parse(block.at) < Date.parse(event.at))) {
        entries.push(ledgerSchema.parse({ kind: 'false-escalation', pr: pr.number, rule: event.rule, label: event.label, at: event.at, by: event.by }));
      }
    }
  }
  return [...new Map(entries.map((entry) => [identity(entry), entry])).values()];
}
export interface Proposal { action: 'soften' | 'prune' | 'sharpen' | 'add'; rule?: string; evidence: number[]; rationale: string }
export interface Evaluation { proposals: Proposal[]; markdown: string }
export function evaluate(ledger: LedgerEntry[], rules: { id: string; mode: Mode }[], config: GuardrailsConfig, options: { since?: string; now: Date }): Evaluation {
  const now = options.now.getTime();
  const since = options.since ?? `${config.evaluation.windowDays}d`;
  const days = /^(\d+)d$/.exec(since);
  const start = days ? now - Number(days[1]) * 86400000 : Date.parse(since);
  if (!Number.isFinite(start) || !Number.isFinite(now) || start > now) throw new Error('Invalid evaluation window');
  const entries = ledger.map((e) => ledgerSchema.parse(e)).filter((e) => Date.parse(e.at) >= start && Date.parse(e.at) <= now);
  const proposals: Proposal[] = [];
  const modes = new Map(rules.map((r) => [r.id, config.rules[r.id] ?? r.mode]));
  for (const [rule, mode] of Object.entries(config.rules)) modes.set(rule, mode);
  const named = new Set([...modes.keys(), ...entries.flatMap((e) => e.kind === 'escaped-bug' ? e.rules : [e.rule])]);
  const evidence = (rows: LedgerEntry[]) => [...new Set(rows.map((e) => e.pr))].sort((a, b) => a - b);
  for (const rule of [...named].sort()) {
    const hits = entries.filter((e) => e.kind === 'escaped-bug' ? e.rules.includes(rule) : e.rule === rule);
    const falseAlarms = hits.filter((e) => e.kind === 'false-escalation');
    const escaped = hits.filter((e) => e.kind === 'escaped-bug');
    if (evidence(falseAlarms).length >= config.evaluation.minFalseEscalations)
      proposals.push({ action: modes.get(rule) === 'block' ? 'soften' : 'prune', rule, evidence: evidence(falseAlarms), rationale: 'Repeated approved escalations need human review.' });
    if (escaped.length) proposals.push({ action: 'sharpen', rule, evidence: evidence(escaped), rationale: 'Escaped bugs identify a coverage gap.' });
    if (!hits.length && (modes.get(rule) === 'warn' || modes.get(rule) === 'off'))
      proposals.push({ action: 'prune', rule, evidence: [], rationale: 'No ledger hits in this window; verify usefulness before removal.' });
  }
  const unattributed = entries.filter((e) => e.kind === 'escaped-bug' && e.rules.length === 0);
  if (unattributed.length) proposals.push({ action: 'add', evidence: evidence(unattributed), rationale: 'Unattributed escaped bugs need a candidate rule.' });
  return { proposals, markdown: '# Guardrails evaluation\n\nProposals only; human approval required.\n\n' + proposals.map((p) => `- ${p.action} ${p.rule ?? 'new rule'}: ${p.rationale} PRs: ${p.evidence.join(', ') || 'none'}`).join('\n') + '\n' };
}
export function evaluateCached(cwd: string, rules: { id: string; mode: Mode }[], config: GuardrailsConfig, options: { since?: string; now: Date; cache?: string }): Evaluation {
  const file = resolve(cwd, config.evaluation.ledgerFile);
  const content = existsSync(file) ? readFileSync(file, 'utf8') : '';
  const key = createHash('sha256').update(JSON.stringify([content, rules, config.rules, config.evaluation, options.since, options.now.toISOString()])).digest('hex');
  const cached = resolve(cwd, options.cache ?? 'node_modules/.cache/guardrails', `evaluate-${key}.json`);
  if (existsSync(cached)) return JSON.parse(readFileSync(cached, 'utf8')) as Evaluation;
  const result = evaluate(readLedger(file), rules, config, options);
  mkdirSync(dirname(cached), { recursive: true });
  writeFileSync(cached, JSON.stringify(result));
  return result;
}
