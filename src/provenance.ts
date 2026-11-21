import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { z } from 'zod';
import type { GuardrailsConfig } from './config.js';

const text = z.string().min(1);
const sha = z.string().regex(/^[a-f0-9]{40}(?:[a-f0-9]{24})?$/);
const check = z.object({ rule: text, mode: z.enum(['block', 'warn', 'off']), count: z.number().int().nonnegative() }).strict();
const override = z.object({ label: text, by: z.string(), at: z.string(), reason: z.string() }).strict();
const schema = z.object({
  testedSha: sha, parents: z.array(sha).min(1), prHeadSha: sha.nullable(),
  report: z.object({ path: text, sha256: z.string().regex(/^[a-f0-9]{64}$/) }).strict(),
  guardrailsVersion: text, checks: z.array(check), ok: z.boolean(), overrides: z.array(override), generatedAt: z.string().datetime(),
}).strict().refine((r) => r.parents[0] === r.testedSha);
export type Provenance = z.infer<typeof schema>;
export interface LabelEvent { event: string; label?: { name: string }; actor?: { login: string }; created_at?: string }
export interface ReasonComment { body: string; user: { login: string } }
const hash = (data: string | Buffer) => createHash('sha256').update(data).digest('hex');

export function overridesFrom(events: LabelEvent[], comments: ReasonComment[], labels: string[], prefix: string): Provenance['overrides'] {
  const reasons = new Map<string, string>();
  for (const comment of comments) {
    for (const line of comment.body.split(/\r?\n/)) {
      const match = /^guardrails-reason: (\S+) (.+)$/.exec(line);
      if (match) reasons.set(match[1], match[2]);
    }
  }
  return [...new Set(labels)].filter((label) => label.startsWith(prefix)).map((label) => {
    const event = events.filter((e) => e.event === 'labeled' && e.label?.name === label)
      .sort((a, b) => (a.created_at ?? '').localeCompare(b.created_at ?? '')).at(-1);
    return { label, by: event?.actor?.login ?? '', at: event?.created_at ?? '', reason: reasons.get(label) ?? '' };
  });
}

export interface RecordOptions {
  cwd: string;
  report: string;
  provenance: GuardrailsConfig['provenance'];
  head?: string;
  env?: NodeJS.ProcessEnv;
  out?: string;
  events?: LabelEvent[];
  comments?: ReasonComment[];
  labels?: string[];
  clock?: () => Date;
  git?: (args: string[], cwd: string) => string;
}

export function record(opts: RecordOptions): Provenance {
  const git = opts.git ?? ((args, cwd) => execFileSync('git', args, { cwd, encoding: 'utf8' }));
  const testedSha = git(['rev-parse', 'HEAD'], opts.cwd).trim();
  const parents = git(['rev-list', '--parents', '-n', '1', 'HEAD'], opts.cwd).trim().split(/\s+/);
  const reportPath = resolve(opts.cwd, opts.report);
  const bytes = readFileSync(reportPath);
  const report = z.object({ ok: z.boolean(), findings: z.array(z.object({ rule: text, mode: check.shape.mode })) }).parse(JSON.parse(bytes.toString('utf8')));
  const checks: Provenance['checks'] = [];
  for (const finding of report.findings) {
    const existing = checks.find((c) => c.rule === finding.rule && c.mode === finding.mode);
    if (existing) existing.count++;
    else checks.push({ rule: finding.rule, mode: finding.mode, count: 1 });
  }
  const result = schema.parse({
    testedSha, parents, prHeadSha: opts.head ?? opts.env?.GITHUB_HEAD_SHA ?? null,
    report: { path: reportPath, sha256: hash(bytes) },
    guardrailsVersion: JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')).version,
    checks, ok: report.ok,
    overrides: overridesFrom(opts.events ?? [], opts.comments ?? [], opts.labels ?? [], opts.provenance.overrideLabelPrefix),
    generatedAt: (opts.clock ?? (() => new Date()))().toISOString(),
  });
  const out = resolve(opts.cwd, opts.out ?? opts.provenance.out);
  if (out === reportPath) throw new Error('Provenance output must differ from the report');
  mkdirSync(dirname(out), { recursive: true });
  const temp = mkdtempSync(join(dirname(out), '.provenance-'));
  try {
    const file = join(temp, 'record.json');
    writeFileSync(file, JSON.stringify(result, null, 2) + '\n');
    renameSync(file, out);
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
  return result;
}

export function marker(record: Provenance): string {
  const value = schema.parse(record);
  const payload = JSON.stringify({ record: value, sha256: hash(JSON.stringify(value)) });
  return `<!-- guardrails:provenance v1 ${Buffer.from(payload).toString('base64url')} -->`;
}

export function parseMarker(text: string): Provenance | null {
  try {
    const matches = [...text.matchAll(/<!-- guardrails:provenance v1 ([A-Za-z0-9_-]+) -->/g)];
    if (matches.length !== 1) return null;
    const encoded = matches[0][1];
    const bytes = Buffer.from(encoded, 'base64url');
    if (bytes.toString('base64url') !== encoded) return null;
    const payload = z.object({ record: schema, sha256: z.string() }).strict().parse(JSON.parse(bytes.toString('utf8')));
    return hash(JSON.stringify(payload.record)) === payload.sha256 ? payload.record : null;
  } catch {
    return null;
  }
}

export function verify(input: unknown, head: string, reportFile?: string): { ok: boolean; violations: { rule: string; message: string }[] } {
  try {
    const value = schema.parse(typeof input === 'string' ? JSON.parse(readFileSync(input, 'utf8')) : input);
    if (head !== value.testedSha && !(value.parents.length === 3 && value.parents[2] === head)) throw new Error('Commit mismatch');
    if (hash(readFileSync(reportFile ?? value.report.path)) !== value.report.sha256) throw new Error('Report mismatch');
    return { ok: true, violations: [] };
  } catch {
    return { ok: false, violations: [{ rule: 'PROV-1', message: 'Missing, invalid or mismatched provenance or report' }] };
  }
}

export async function publish(record: Provenance, uploader: (record: Provenance) => Promise<unknown>): Promise<string | undefined> {
  try {
    await uploader(structuredClone(record));
    return undefined;
  } catch (error) {
    return `Provenance upload failed: ${String(error)}`;
  }
}
