import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it, vi } from 'vitest';
import { run } from '../src/runner.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const seen = vi.hoisted(() => ({ ctx: [] as unknown[] }));
const out = vi.hoisted(() => (rule: string, ctx?: unknown) => {
  if (ctx) seen.ctx.push(ctx);
  return { violations: [{ rule, file: `${rule}.ts`, message: `${rule} found`, fingerprint: rule }], metrics: [{ name: rule, value: 1 }], ...(rule === 'JRN-1' ? { journeys: [{ id: 'J1' }] } : {}) };
});
vi.mock('../src/checks/docs.js', () => ({ checkDocs: (ctx: unknown) => out('DOC-2', ctx) }));
vi.mock('../src/checks/runbook.js', () => ({ checkRunbook: () => out('RUN-1') }));
vi.mock('../src/checks/ci.js', () => ({ checkCi: () => out('CI-1') }));
vi.mock('../src/checks/suite.js', () => ({ checkSuite: () => out('SUITE-1') }));
vi.mock('../src/checks/setup.js', () => ({ checkSetup: () => out('TEST-1') }));
vi.mock('../src/checks/tests.js', () => ({ checkTests: () => out('TEST-2') }));
vi.mock('../src/checks/lint.js', () => ({ checkLint: async () => out('ARCH-1') }));
vi.mock('../src/checks/arch.js', () => ({ checkArch: async () => out('ARCH-2') }));
vi.mock('../src/checks/dup.js', () => ({ checkDuplication: () => out('ARCH-3') }));
vi.mock('../src/checks/gaming.js', () => ({ checkGaming: () => out('TEST-5') }));
vi.mock('../src/flags.js', () => ({ checkFlags: () => out('FLAG-1') }));
vi.mock('../src/checks/scope.js', () => ({ checkScope: () => out('SCOPE-1') }));
vi.mock('../src/checks/journeys.js', () => ({ checkJourneys: () => out('JRN-1') }));
vi.mock('../src/checks/failbase.js', () => ({ checkFailsOnBase: () => out('TEST-11') }));
vi.mock('../src/checks/clock.js', () => ({ checkClock: () => out('TEST-9') }));
vi.mock('../src/checks/mutation.js', () => ({ checkMutation: () => out('TEST-8') }));

const GROUPS = [
  ['docs', ['DOC-2']], ['runbook', ['RUN-1']], ['ci', ['CI-1']], ['suite', ['SUITE-1']], ['setup', ['TEST-1']], ['tests', ['TEST-2']],
  ['lint', ['ARCH-1']], ['arch', ['ARCH-2', 'ARCH-3']], ['gaming', ['TEST-5']], ['flags', ['FLAG-1']], ['scope', ['SCOPE-1']],
  ['journeys', ['JRN-1']], ['failbase', ['TEST-11']], ['clock', ['TEST-9']], ['mutation', ['TEST-8']],
] as const;
const ALL = GROUPS.flatMap(([, rules]) => rules);
const RUNTIME = ['failbase', 'clock', 'mutation'];
const RUNTIME_RULES = ['TEST-11', 'TEST-9', 'TEST-8'];
const config = { ...BASE_CONFIG, rules: Object.fromEntries(ALL.map((r) => [r, 'block'])), feedback: { enabled: true, logFile: 'out/failures.jsonl' } };
const logged = (dir: string) => readFileSync(join(dir, 'out/failures.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l).rule);

it.each(GROUPS)('FEED-01 runner logs blocking failures for the selected check group %s', async (group, rules) => {
  const r = repo({}, config);
  const runtime = { failsOnBase: true, clockShift: true, mutation: true };
  const rep = await run({ cwd: r.dir, only: [group], ...runtime });
  expect(rep.findings.map((f) => f.rule)).toEqual([...rules]);
  expect(rep.metrics.map((m) => m.name)).toEqual([...rules]);
  expect(rep.counts).toEqual({ block: rules.length, warn: 0, known: 0 });
  expect(rep.ok).toBe(false);
  expect('journeys' in rep).toBe(group === 'journeys');
  expect(logged(r.dir)).toEqual([...rules]);
  const off = await run({ cwd: r.dir, only: [group] });
  expect(off.findings.map((f) => f.rule)).toEqual(RUNTIME.includes(group) ? [] : [...rules]);
});

it('FEED-01 runner logs every group, passes options through, reports stale baseline entries and a bad config as CFG-1', async () => {
  const r = repo({}, { ...config, baselineFile: 'b.json' });
  writeFileSync(join(r.dir, 'b.json'), JSON.stringify({ version: 1, entries: [{ rule: 'DOC-2', file: 'DOC-2.ts', fingerprint: 'DOC-2' }, { rule: 'DOC-2', file: 'x', fingerprint: 'gone' }] }));
  seen.ctx.length = 0;
  const rep = await run({ cwd: r.dir, base: 'origin/main', labels: ['a'], failsOnBase: true, clockShift: true, mutation: true, knip: false, mutate: ['m'], specs: ['S-01'] });
  expect(rep.findings.map((f) => [f.rule, f.known])).toEqual(ALL.map((rule) => [rule, rule === 'DOC-2']));
  expect([rep.base, rep.staleBaseline, rep.counts]).toEqual(['origin/main', 1, { block: ALL.length - 1, warn: 0, known: 1 }]);
  expect(logged(r.dir)).toEqual(ALL.filter((rule) => rule !== 'DOC-2'));
  const ctx = seen.ctx[0] as { base: string; labels: string[]; options: unknown };
  expect([ctx.base, ctx.labels, ctx.options]).toEqual(['origin/main', ['a'], { clockShift: true, mutation: true, knip: false, mutate: ['m'], failsOnBase: true, specs: ['S-01'] }]);
  await run({ cwd: r.dir });
  const plain = seen.ctx[1] as { base: unknown; labels: unknown; options: unknown };
  expect([plain.base, plain.labels, plain.options]).toEqual([null, [], { clockShift: false, mutation: false, knip: true, mutate: undefined, failsOnBase: false, specs: undefined }]);
  expect((await run({ cwd: r.dir, only: ['docs'] })).staleBaseline).toBe(0);
  r.write({ 'guardrails.config.json': '{"version":1,"oops":true}' });
  for (const base of [undefined, 'b']) {
    const rep = await run({ cwd: r.dir, base });
    expect(rep).toEqual({ ok: false, base: base ?? null, findings: [{ ...rep.findings[0], rule: 'CFG-1', file: 'guardrails.config.json', mode: 'block', known: false }], metrics: [], counts: { block: 1, warn: 0, known: 0 }, staleBaseline: 0 });
    expect(rep.findings[0].message).toContain('oops');
  }
  const off = repo({}, { ...config, feedback: { enabled: false, logFile: 'out/failures.jsonl' } });
  await run({ cwd: off.dir });
  expect(() => logged(off.dir)).toThrow();
  r.write({ 'guardrails.config.json': JSON.stringify({ ...config, baselineFile: 'b.json' }), 'out/failures.jsonl': '' });
  await run({ cwd: r.dir });
  const fresh = ALL.filter((rule) => !RUNTIME_RULES.includes(rule) && rule !== 'DOC-2');
  expect(logged(r.dir)).toEqual(fresh);
});
