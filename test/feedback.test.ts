import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { appendFailures, clusterFailures, draftRule, draftingPrompt, feedbackDraft, readFailures, tagFailures, type Failure } from '../src/feedback.js';
import { config, now, fixture, expect, it, beforeEach, afterEach, vi } from './helpers/evaluate.js';
import type { Finding } from '../src/types.js';
const dir = fixture();
const finding: Finding = { rule: 'ARCH-5', file: 'src/a.ts', message: 'Forbidden "fetch" at src/a.ts:23', mode: 'block', known: false, fingerprint: 'a' };
const failure: Failure = { rule: finding.rule, file: finding.file, agent: 'eval', message: finding.message, at: now.toISOString() };
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(now); });
afterEach(() => { vi.useRealTimers(); vi.unstubAllEnvs(); });
it.each([
  ['block', false, true], ['block', true, false], ['warn', false, false], ['off', false, false],
] as const)('FEED-01 tags only new blocking findings %#', (mode, known, included) => {
  expect(tagFailures({ findings: [{ ...finding, mode, known }] }, 'eval', () => now)).toEqual(included ? [failure] : []);
});
it('FEED-01 resolves agent identity with explicit precedence', () => {
  vi.stubEnv('GUARDRAILS_AGENT_NAME', 'temporary');
  delete process.env.GUARDRAILS_AGENT_NAME;
  expect(tagFailures({ findings: [finding] })[0]).toEqual({ ...failure, agent: 'unknown' });
  vi.stubEnv('GUARDRAILS_AGENT_NAME', 'environment');
  expect(tagFailures({ findings: [finding] })[0]).toEqual({ ...failure, agent: 'environment' });
  expect(tagFailures({ findings: [finding] }, 'explicit')[0]).toEqual({ ...failure, agent: 'explicit' });
});
it('FEED-01 appends only when enabled and preserves existing failures', () => {
  const cfg = { ...config, feedback: { ...config.feedback, logFile: 'nested/failures.jsonl' } };
  const file = join(dir, cfg.feedback.logFile);
  appendFailures(dir, cfg, { findings: [finding] }, 'eval');
  expect(existsSync(file)).toBe(false);
  cfg.feedback.enabled = true;
  appendFailures(dir, cfg, { findings: [] }, 'eval');
  expect(existsSync(file)).toBe(false);
  appendFailures(dir, cfg, { findings: [finding] }, 'eval');
  appendFailures(dir, cfg, { findings: [finding] }, 'second');
  expect(readFileSync(file, 'utf8')).toBe(JSON.stringify(failure) + '\n' + JSON.stringify({ ...failure, agent: 'second' }) + '\n');
  expect(readFailures(file)).toEqual([failure, { ...failure, agent: 'second' }]);
});
it('FEED-01 reads CLI agent option before environment', () => {
  const old = process.argv;
  vi.stubEnv('GUARDRAILS_AGENT_NAME', 'environment');
  try {
    process.argv = ['node', 'guardrails', 'check', '--agent-name', 'cli'];
    const cfg = { ...config, feedback: { ...config.feedback, enabled: true, logFile: 'cli.jsonl' } };
    appendFailures(dir, cfg, { findings: [finding] });
    expect(readFailures(join(dir, 'cli.jsonl'))).toEqual([{ ...failure, agent: 'cli' }]);
  } finally { process.argv = old; }
});
it('FEED-01 validates logs and tolerates empty CRLF lines', () => {
  expect(readFailures(join(dir, 'missing'))).toEqual([]);
  mkdirSync(dir, { recursive: true });
  const file = join(dir, 'read.jsonl');
  writeFileSync(file, '\r\n \r\n' + JSON.stringify(failure) + '\r\n');
  expect(readFailures(file)).toEqual([failure]);
  writeFileSync(file, JSON.stringify({ ...failure, at: 'bad' }));
  let failed = false;
  try { readFailures(file); } catch { failed = true; }
  expect(failed).toBe(true);
});
it.each([
  ['Forbidden "fetch" at src/a.ts:23', 'Forbidden \'send\' at C:\\project\\b.ts:8'],
  ['Forbidden `fetch` at /project/a.ts:23', 'Forbidden "send" at lib/b.ts:8'],
])('FEED-02 clusters normalized literals paths and digits %#', (first, second) => {
  const entries = [{ ...failure, message: first }, { ...failure, message: second }];
  expect(clusterFailures(entries, 2)).toEqual([{ rule: 'ARCH-5', message: 'Forbidden <quoted> at <path>', entries }]);
  expect(clusterFailures(entries, 3)).toEqual([]);
  expect(clusterFailures([entries[0], { ...entries[1], rule: 'OTHER' }], 2)).toEqual([]);
  expect(clusterFailures([entries[0], { ...entries[1], message: 'Different 12' }], 2)).toEqual([]);
  expect(clusterFailures([{ ...failure, message: 'Found 12 defects and 34 warnings' }], 1)[0].message).toBe('Found <number> defects and <number> warnings');
});
it.each(['"fetch"', "'fetch'", '`fetch`'])('FEED-02 drafts restricted strings for quoted evidence %s', (literal) => {
  expect(draftRule({ rule: 'ARCH-5', message: 'normalised', entries: [{ ...failure, message: `Forbidden ${literal}` }] })).toEqual({
    kind: 'restrictedStrings', snippet: '{"pattern":"fetch","message":"ARCH-5: 1 repeated failures require human review."}', rationale: 'ARCH-5: 1 repeated failures require human review.',
  });
});
it('FEED-02 drafts an explicit ESLint placeholder and report only', () => {
  const entry = { ...failure, message: 'Unexpected syntax' };
  const candidate = { kind: 'eslint', snippet: '{"no-restricted-syntax":["error",{"selector":"REPLACE_WITH_REVIEWED_SELECTOR","message":"ARCH-5: 1 repeated failures require human review."}]}', rationale: 'ARCH-5: 1 repeated failures require human review.' };
  expect(draftRule({ rule: 'ARCH-5', message: entry.message, entries: [entry] })).toEqual(candidate);
  expect(draftingPrompt).toBe('Draft a candidate lint rule from repeated blocking failures. Return a restrictedStrings or ESLint no-restricted-syntax proposal with a one-line rationale for human approval; never apply it.');
  mkdirSync(dir, { recursive: true });
  const file = join(dir, 'draft.jsonl');
  writeFileSync(file, JSON.stringify(entry));
  expect(feedbackDraft(file, 1)).toEqual({ candidates: [candidate], markdown: '# Guardrails feedback\n\n' + draftingPrompt + '\n\n- eslint: ARCH-5: 1 repeated failures require human review.\n\n```json\n' + candidate.snippet + '\n```\n' });
  expect(readFileSync(file, 'utf8')).toBe(JSON.stringify(entry));
  expect(feedbackDraft(file, 2)).toEqual({ candidates: [], markdown: '# Guardrails feedback\n\n' + draftingPrompt + '\n\n\n' });
});
import { run } from '../src/runner.js';
import { repo } from './helpers/repo.js';
it('FEED-01 runner persists blocking findings when feedback is enabled', async () => {
  vi.stubEnv('GUARDRAILS_AGENT_NAME', 'eval');
  const r = repo({}, { version: 1, feedback: { enabled: true, logFile: 'failures.jsonl' } });
  r.remove('.github/CODEOWNERS');
  const report = await run({ cwd: r.dir, only: ['setup'], knip: false });
  expect(report.ok).toBe(false);
  expect(readFailures(join(r.dir, 'failures.jsonl'))).toEqual(tagFailures(report, 'eval', () => now));
  expect(readFailures(join(r.dir, 'failures.jsonl')).map((f) => f.rule)).toEqual(['CFG-2']);
});

