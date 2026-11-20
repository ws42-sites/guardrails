import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { evaluate, evaluateCached, ledgerAdd, ledgerImport, ledgerSchema, readLedger, type LedgerEntry } from '../src/evaluate.js';
import { config, now, fixture, expect, it } from './helpers/evaluate.js';
const dir = fixture();
const at = now.toISOString();
const bug: LedgerEntry = { kind: 'escaped-bug', pr: 2, issue: 3, rules: ['A'], at };
const alarm: LedgerEntry = { kind: 'false-escalation', pr: 4, rule: 'A', label: 'guardrails:flags-approved', at };
it.each([
  [{ ...bug, pr: 0 }, false], [{ ...bug, issue: 1.5 }, false], [{ ...bug, rules: [''] }, false],
  [{ ...bug, at: 'yesterday' }, false], [{ ...alarm, label: '' }, false], [{ ...alarm, rule: '' }, false],
  [{ ...bug, extra: true }, false], [{ ...bug, by: '' }, false], [bug, true], [alarm, true],
] as const)('EVAL-02 validates ledger entries %#', (entry, valid) => {
  expect(ledgerSchema.safeParse(entry).success).toBe(valid);
});
it('EVAL-02 persists deduplicated JSONL and handles missing final newline', () => {
  const file = join(dir, 'nested', 'ledger.jsonl');
  expect(readLedger(file)).toEqual([]);
  expect(ledgerAdd(file, bug)).toBe(true);
  expect(ledgerAdd(file, { ...bug, at: '2026-01-09T00:00:00Z' })).toBe(false);
  writeFileSync(file, JSON.stringify(bug));
  expect(ledgerAdd(file, alarm)).toBe(true);
  expect(ledgerAdd(file, { ...alarm, label: 'another' })).toBe(false);
  expect(readFileSync(file, 'utf8')).toBe(JSON.stringify(bug) + '\n' + JSON.stringify({ pr: 4, at, kind: 'false-escalation', rule: 'A', label: 'guardrails:flags-approved' }) + '\n');
  writeFileSync(file, '\r\n  \r\n' + JSON.stringify(bug) + '\r\n');
  expect(readLedger(file)).toEqual([bug]);
  writeFileSync(file, '');
  expect(ledgerAdd(file, alarm)).toBe(true);
  expect(readFileSync(file, 'utf8')).toBe(JSON.stringify({ pr: 4, at, kind: 'false-escalation', rule: 'A', label: 'guardrails:flags-approved' }) + '\n');
});
it('EVAL-02 imports merged issue evidence and strictly later approvals', () => {
  const blocked = { event: 'blocked', at: '2026-01-07T00:00:00Z', rule: 'A' };
  const approved = { event: 'labeled', at, rule: 'A', label: alarm.label, by: 'josh' };
  expect(ledgerImport([
    { number: 2, mergedAt: '2026-01-07T00:00:00Z', linkedIssues: [{ number: 3, labels: [{ name: config.evaluation.escapedLabel }], at, rules: ['A'] }], events: [blocked, approved, approved] },
    { number: 5, mergedAt: null, linkedIssues: [{ number: 7, labels: [config.evaluation.escapedLabel], at }], events: [] },
    { number: 6, mergedAt: at, linkedIssues: [{ number: 8, labels: [], at }], events: [approved] },
  ], config)).toEqual([bug, { ...alarm, pr: 2, by: 'josh' }]);
});
it.each([
  ['guardrails:flags-approved', 'A', '2026-01-07T00:00:00Z', 1],
  ['other:flags-approved', 'A', '2026-01-07T00:00:00Z', 0],
  ['guardrails:flags', 'A', '2026-01-07T00:00:00Z', 0],
  [undefined, 'A', '2026-01-07T00:00:00Z', 0],
  ['guardrails:flags-approved', 'B', '2026-01-07T00:00:00Z', 0],
  ['guardrails:flags-approved', 'A', at, 0],
  ['guardrails:flags-approved', 'A', '2026-01-09T00:00:00Z', 0],
])('EVAL-02 requires a matching earlier block %#', (label, rule, blockedAt, count) => {
  expect(ledgerImport([{ number: 4, mergedAt: null, events: [{ event: 'blocked', at: blockedAt, rule }, { event: 'labeled', at, rule: 'A', label }] }], config).length).toBe(count);
});
it.each(['2026-01-07T00:00:00Z', at, '2026-01-09T00:00:00Z'])('EVAL-02 requires escaped issue after merge %s', (issueAt) => {
  expect(ledgerImport([{ number: 2, mergedAt: at, linkedIssues: [{ number: 3, labels: [config.evaluation.escapedLabel], at: issueAt }] }], config)).toEqual(issueAt < at ? [] : [{ ...bug, rules: [], at: issueAt }]);
});
it('EVAL-01 proposes evidence-based changes without changing config', () => {
  const cfg = { ...config, rules: { B: 'off' as const, C: 'warn' as const, D: 'block' as const } };
  const original = JSON.stringify(cfg);
  const result = evaluate([bug, { ...bug, pr: 7, issue: 8, rules: [] }, alarm, { ...alarm, pr: 5 }, { ...alarm, pr: 6 }, alarm], [{ id: 'A', mode: 'block' }], cfg, { now });
  expect(result.proposals).toEqual([
    { action: 'soften', rule: 'A', evidence: [4, 5, 6], rationale: 'Repeated approved escalations need human review.' },
    { action: 'sharpen', rule: 'A', evidence: [2], rationale: 'Escaped bugs identify a coverage gap.' },
    { action: 'prune', rule: 'B', evidence: [], rationale: 'No ledger hits in this window; verify usefulness before removal.' },
    { action: 'prune', rule: 'C', evidence: [], rationale: 'No ledger hits in this window; verify usefulness before removal.' },
    { action: 'add', evidence: [7], rationale: 'Unattributed escaped bugs need a candidate rule.' },
  ]);
  expect(result.markdown).toBe('# Guardrails evaluation\n\nProposals only; human approval required.\n\n- soften A: Repeated approved escalations need human review. PRs: 4, 5, 6\n- sharpen A: Escaped bugs identify a coverage gap. PRs: 2\n- prune B: No ledger hits in this window; verify usefulness before removal. PRs: none\n- prune C: No ledger hits in this window; verify usefulness before removal. PRs: none\n- add new rule: Unattributed escaped bugs need a candidate rule. PRs: 7\n');
  expect(JSON.stringify(cfg)).toBe(original);
});
it.each([
  ['2025-12-31T23:59:59Z', '7d', 0], ['2026-01-01T00:00:00Z', '7d', 1],
  ['2026-01-08T00:00:00Z', '2026-01-08T00:00:00Z', 1], ['2026-01-08T00:00:01Z', '7d', 0],
])('EVAL-01 respects inclusive window boundaries %#', (date, since, count) => {
  expect(evaluate([{ ...bug, at: date }], [], config, { now, since }).proposals.length).toBe(count);
});
it.each(['invalid', '2026-01-09', '-1d'])('EVAL-01 rejects invalid windows %s', (since) => {
  let message = '';
  try { evaluate([], [], config, { now, since }); } catch (e) { message = (e as Error).message; }
  expect(message).toBe('Invalid evaluation window');
});
it.each(['block', 'warn', 'off'] as const)('EVAL-01 uses configured modes and threshold %s', (mode) => {
  const cfg = { ...config, rules: { A: mode }, evaluation: { ...config.evaluation, minFalseEscalations: 2 } };
  expect(evaluate([alarm, alarm], [{ id: 'A', mode: 'block' }], cfg, { now }).proposals).toEqual([]);
  expect(evaluate([alarm, { ...alarm, pr: 1 }], [{ id: 'A', mode: 'block' }], cfg, { now }).proposals).toEqual([{ action: mode === 'block' ? 'soften' : 'prune', rule: 'A', evidence: [1, 4], rationale: 'Repeated approved escalations need human review.' }]);
});
it('EVAL-01 caches exact inputs and invalidates evidence modes and window', () => {
  const cfg = { ...config, evaluation: { ...config.evaluation, ledgerFile: 'ledger.jsonl' } };
  const options = { now, cache: 'cache' };
  const result = evaluateCached(dir, [], cfg, options);
  expect(result).toEqual(evaluate([], [], cfg, { now }));
  const file = join(dir, 'cache', readdirSync(join(dir, 'cache'))[0]);
  writeFileSync(file, JSON.stringify({ proposals: [], markdown: 'cached' }));
  expect(evaluateCached(dir, [], cfg, options)).toEqual({ proposals: [], markdown: 'cached' });
  ledgerAdd(join(dir, 'ledger.jsonl'), bug);
  expect(evaluateCached(dir, [], cfg, options)).toEqual(evaluate([bug], [], cfg, options));
  evaluateCached(dir, [{ id: 'A', mode: 'warn' }], cfg, options);
  evaluateCached(dir, [], cfg, { ...options, since: '1d' });
  evaluateCached(dir, [], cfg, { ...options, now: new Date('2026-01-09T00:00:00Z') });
  expect(readdirSync(join(dir, 'cache')).length).toBe(5);
  expect(evaluateCached(dir, [], cfg, { now })).toEqual(evaluate([bug], [], cfg, { now }));
  expect(readdirSync(join(dir, 'node_modules/.cache/guardrails')).length).toBe(1);
});
