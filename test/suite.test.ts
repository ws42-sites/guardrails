import { describe, expect, it } from 'vitest';
import { checkSuite } from '../src/checks/suite.js';
import { auditSuite, ratchetSuite, regressions, toBaseline, writeSuiteBaseline } from '../src/test-suite.js';
import { shardFiles } from '../src/shard.js';
import { listFiles } from '../src/util.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const ON = { ...BASE_CONFIG, testSuite: { enabled: true, maxLines: 20 } };
const body = (n: number) => `it('CORE-01 case ${n}', () => {\n  const x = add(${n}, 1);\n  expect(x).toBe(${n + 1});\n  expect(typeof x).toBe('number');\n});\n`;
const BAD = [
  "it.only('CORE-01 a', () => { expect(1).toBe(1); });",
  "it.skip('CORE-01 b', () => {});",
  "it.skip('CORE-01 c', () => {}); // flaky upstream, TEST-SKIP: #123",
  "const src = readFileSync('src/core/add.ts', 'utf8');",
  'await new Promise((r) => setTimeout(r, 500));',
  'expect(stamp).toBeLessThan(Date.now());',
].join('\n');

describe('SUITE-01 check-tests finds the banned patterns and blocks only regressions vs the baseline', () => {
  it('counts each kind per file, ratchets down, and reports growth past the baseline', () => {
    const r = repo({ 'src/core/add.ts': 'export const add = (a: number, b: number) => a + b;\n', 'src/core/a.test.ts': `${BAD}\n${body(1)}${body(2)}${body(3)}` }, ON);
    const a = auditSuite(r.dir, r.ctx().config, listFiles(r.dir));
    const base = toBaseline(a, 20);
    expect([base.byKind, a.totals.ratio]).toEqual([{ oversize: 1, focused: 1, skip: 1, 'source-text': 1, sleep: 1, clock: 1, 'copy-paste': 1 }, 21]);
    expect(regressions(a, base)).toEqual([]);
    r.write({ 'src/core/a.test.ts': `${BAD}\n${BAD}\n${body(1)}${body(2)}${body(3)}` });
    const worse = auditSuite(r.dir, r.ctx().config, listFiles(r.dir));
    expect(regressions(worse, base).map((x) => x.key).sort()).toEqual(['clock|src/core/a.test.ts', 'focused|src/core/a.test.ts', 'oversize|src/core/a.test.ts', 'skip|src/core/a.test.ts', 'sleep|src/core/a.test.ts', 'source-text|src/core/a.test.ts']);
    r.write({ 'src/core/a.test.ts': body(1) });
    const fixed = toBaseline(auditSuite(r.dir, r.ctx().config, listFiles(r.dir)), 20);
    expect(ratchetSuite(base, fixed).byKind).toEqual({ oversize: 0 });
  });
});

describe('SUITE-02 guardrails check gates suite regressions and baseline growth', () => {
  it('blocks without a baseline, passes with one, and blocks a grown baseline in diff mode', () => {
    const r = repo({ 'src/core/a.test.ts': `${BAD}\n` }, ON);
    expect(checkSuite(r.ctx()).violations.map((v) => v.rule)).toEqual(['SUITE-6', 'SUITE-1', 'SUITE-1', 'SUITE-1', 'SUITE-3', 'SUITE-4']);
    writeSuiteBaseline(r.dir, '.guardrails/test-suite-baseline.json', toBaseline(auditSuite(r.dir, r.ctx().config, listFiles(r.dir)), 20));
    r.commit();
    expect(checkSuite(r.ctx()).violations).toEqual([]);
    r.write({ 'src/core/b.test.ts': 'it.only("CORE-01 x", () => {});\n' });
    writeSuiteBaseline(r.dir, '.guardrails/test-suite-baseline.json', toBaseline(auditSuite(r.dir, r.ctx().config, listFiles(r.dir)), 20));
    expect(checkSuite(r.ctx('HEAD')).violations.map((v) => `${v.rule} ${v.message.split('.')[0]}`)).toEqual(['SUITE-6 test-suite baseline grew (focused 1 -> 2)']);
  });
});

describe('SUITE-03 guardrails shard splits the suite into balanced, complete shards', () => {
  it('covers every test file once across shards, heaviest files spread first', () => {
    const r = repo({ 'src/a.test.ts': 'x\n'.repeat(90), 'src/b.test.ts': 'x\n'.repeat(50), 'src/c.test.ts': 'x\n'.repeat(40), 'src/d.test.ts': 'x\n' }, BASE_CONFIG);
    const files = listFiles(r.dir);
    const shards = [1, 2].map((i) => shardFiles(r.dir, r.ctx().config, files, i, 2));
    expect(shards).toEqual([['src/a.test.ts', 'src/d.test.ts'], ['src/b.test.ts', 'src/c.test.ts']]);
    expect(() => shardFiles(r.dir, r.ctx().config, files, 3, 2)).toThrow('out of range');
  });
});
