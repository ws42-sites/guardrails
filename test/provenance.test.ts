import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { overridesFrom, publish, record } from '../src/provenance.js';
import { config, fixture, head, instant, tested } from './helpers/provenance.js';

afterEach(() => { vi.useRealTimers(); });

it('PROV-01 binds one atomic record to report bytes, Git and tool version', () => {
  const f = fixture();
  expect(f.calls).toEqual([[['rev-parse', 'HEAD'], f.cwd], [['rev-list', '--parents', '-n', '1', 'HEAD'], f.cwd]]);
  expect(f.value).toEqual({ testedSha: tested, parents: [tested], prHeadSha: null,
    report: { path: join(f.cwd, 'report.json'), sha256: createHash('sha256').update(f.body).digest('hex') },
    guardrailsVersion: JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')).version,
    checks: [{ rule: 'TEST-1', mode: 'block', count: 2 }, { rule: 'TEST-1', mode: 'warn', count: 1 }, { rule: 'ARCH-1', mode: 'warn', count: 1 }],
    ok: false, overrides: [], generatedAt: instant });
  expect(f.saved()).toEqual(f.value);
  expect(readdirSync(f.cwd).sort()).toEqual(['proof.json', 'report.json']);
});

it.each([
  { explicit: head, env: tested, expected: head },
  { explicit: undefined, env: head, expected: head },
  { explicit: undefined, env: undefined, expected: null },
])('PROV-01 honours explicit head then injected environment $expected', ({ explicit, env, expected }) => {
  const f = fixture();
  vi.useFakeTimers();
  vi.setSystemTime(new Date(instant));
  writeFileSync(join(f.cwd, 'report.json'), JSON.stringify({ ok: true, findings: [] }));
  const value = record({ ...f.opts, clock: undefined, head: explicit, env: { GITHUB_HEAD_SHA: env }, out: 'nested/result.json',
    labels: ['guardrails:approved'], events: [{ event: 'labeled', label: { name: 'guardrails:approved' }, actor: { login: 'owner' }, created_at: instant }],
    comments: [{ body: 'guardrails-reason: guardrails:approved reviewed', user: { login: 'owner' } }] });
  expect(value.prHeadSha).toBe(expected);
  expect(value.generatedAt).toBe(instant);
  expect(value.ok).toBe(true);
  expect(value.checks).toEqual([]);
  expect(value.overrides).toEqual([{ label: 'guardrails:approved', by: 'owner', at: instant, reason: 'reviewed' }]);
  expect(JSON.parse(readFileSync(join(f.cwd, 'nested/result.json'), 'utf8'))).toEqual(value);
});

it('PROV-01 extracts current override labels with latest label event and reason', () => {
  const label = 'guardrails:approved';
  const events = [
    { event: 'labeled', label: { name: label }, actor: { login: 'latest' }, created_at: '2026-02-01' },
    { event: 'labeled', label: { name: label }, actor: { login: 'earlier' }, created_at: '2026-01-01' },
    { event: 'unlabeled', label: { name: label }, actor: { login: 'ignored' }, created_at: '2026-03-01' },
    { event: 'labeled', label: { name: 'foreign' }, actor: { login: 'other' }, created_at: '2026-04-01' },
    { event: 'labeled' }, { event: 'labeled', label: { name: label } },
  ];
  expect(overridesFrom(events, [
    { body: `guardrails-reason: ${label} old\r\nnoise`, user: { login: 'earlier' } },
    { body: `guardrails-reason: ${label} final reason\r\nguardrails-reason: guardrails:missing pending`, user: { login: 'latest' } },
  ], [label, label, 'guardrails:missing', 'guardrails:empty', 'foreign', 'xguardrails:no'], 'guardrails:')).toEqual([
    { label, by: 'latest', at: '2026-02-01', reason: 'final reason' },
    { label: 'guardrails:missing', by: '', at: '', reason: 'pending' },
    { label: 'guardrails:empty', by: '', at: '', reason: '' },
  ]);
});

it.each(['invalid-report', 'same-path', 'rename-failure'])('PROV-01 preserves existing proof on %s', (failure) => {
  const f = fixture();
  if (failure === 'invalid-report') writeFileSync(join(f.cwd, 'report.json'), '{}');
  const out = failure === 'same-path' ? 'report.json' : failure === 'rename-failure' ? '.' : undefined;
  let threw = false;
  try { record({ ...f.opts, out }); } catch { threw = true; }
  expect(threw).toBe(true);
  expect(f.saved()).toEqual(f.value);
  expect(readdirSync(f.cwd).sort()).toEqual(['proof.json', 'report.json']);
});

it.each([undefined, new Error('offline'), 'denied'])('PROV-01 defers upload without changing stored or in-memory verdict %s', async (error) => {
  const f = fixture();
  let release: () => void = () => {};
  const gate = new Promise<void>((resolve) => { release = resolve; });
  let completed = false;
  const pending = publish(f.value, async (value) => {
    expect(existsSync(join(f.cwd, config.out))).toBe(true);
    expect(value).toEqual(f.saved());
    value.ok = true;
    await gate;
    completed = true;
    if (error) throw error;
  });
  expect(completed).toBe(false);
  expect(f.value.ok).toBe(false);
  release();
  expect(await pending).toBe(error ? `Provenance upload failed: ${String(error)}` : undefined);
  expect(completed).toBe(true);
  expect(f.saved()).toEqual(f.value);
});
