import { describe, expect, it } from 'vitest';
import { checkTests } from '../src/checks/tests.js';
import { checkArch } from '../src/checks/arch.js';
import { repo, BASE_CONFIG } from './helpers/repo.js';

const test2 = `it('CORE-01 a', () => { expect(add(1, 2)).toBe(3) });\nit('CORE-01 b', () => { expect(add(2, 2)).toBe(4); expect(add(0, 0)).toBe(0) });\n`;
const weakening = (r: ReturnType<typeof repo>, labels: string[] = []) =>
  checkTests(r.ctx('main', labels)).violations.filter((v) => v.rule === 'TEST-7').map((v) => v.message.split('.')[0]);

describe('DIFF-01 deleting base tests needs the label', () => {
  it('blocks deleting a base test without tests-removed-approved, allows it with the label', () => {
    const r = repo({ 'src/core/a.test.ts': test2 }).commit();
    r.write({ 'src/core/a.test.ts': test2.split('\n')[0] + '\n' });
    expect(weakening(r)).toEqual(['deleted base test "CORE-01 b"']);
    const approved = checkTests(r.ctx('main', ['tests-removed-approved'])).violations;
    expect(approved.filter((v) => v.rule === 'TEST-7')).toEqual([]);
    expect(approved.filter((v) => v.rule === 'TEST-10').map((v) => v.message)).toEqual(['test count dropped by 1 (2 on base, 1 now).']);
  });

  it('lets a PR delete tests it added itself, and moves between files are not deletions', () => {
    const r = repo({ 'src/core/a.test.ts': test2 }).commit();
    r.write({ 'src/core/b.test.ts': `it('CORE-01 new', () => { expect(1).toBe(1) });\n` });
    r.write({ 'src/core/b.test.ts': test2 }).remove('src/core/a.test.ts');
    expect(weakening(r)).toEqual([]);
  });
});

describe('DIFF-02 removing expects or loosening matchers is weakening', () => {
  it.each([
    [`it('CORE-01 b', () => { expect(add(2, 2)).toBe(4) });`, '"CORE-01 b" lost 1 expect() call(s)'],
    [`it('CORE-01 b', () => { expect(add(2, 2)).toBeTruthy(); expect(add(0, 0)).toBe(0) });`, '"CORE-01 b" loosened a matcher (toBe, toBe -> toBeTruthy, toBe)'],
  ])('flags %s', (replacement, message) => {
    const r = repo({ 'src/core/a.test.ts': test2 }).commit();
    r.write({ 'src/core/a.test.ts': test2.split('\n')[0] + '\n' + replacement + '\n' });
    expect(weakening(r)).toEqual([message]);
  });

  it('flags lowered thresholds in tool configs and baseline growth', () => {
    const r = repo({ 'vitest.config.ts': 'export default { test: { coverage: { thresholds: { lines: 80 } } } }', '.guardrails/baseline.json': '{"entries":[]}' }).commit();
    r.write({ 'vitest.config.ts': 'export default { test: { coverage: { thresholds: { lines: 50 } } } }', '.guardrails/baseline.json': '{"entries":[{"rule":"DOC-1","fingerprint":"abc"}]}' });
    const v = checkTests(r.ctx('main')).violations.filter((x) => ['TEST-7', 'BASE-1'].includes(x.rule));
    expect(v.map((x) => [x.rule, x.message.split('.')[0]])).toEqual([['TEST-7', 'threshold lines lowered 80 -> 50'], ['BASE-1', 'baseline grew by 1 entry']]);
    expect(checkTests(r.ctx('main', ['guardrails:rebaseline'])).violations.filter((x) => x.rule === 'BASE-1')).toEqual([]);
  });
});

describe('BUDGET-01 test budget is per tier and warn-only', () => {
  it('reports ratios per tier, excludes fixtures, and handles test-only diffs', () => {
    const r = repo({ 'src/core/add.ts': 'export const add = (a: number, b: number) => a + b;\n' }).commit();
    r.write({
      'src/core/sub.ts': 'export const sub = (a: number, b: number) => a - b;\n',
      'src/core/sub.test.ts': `it('CORE-01 sub', () => {\n  expect(sub(3, 1)).toBe(2);\n  expect(sub(1, 3)).toBe(-2);\n});\n`,
      'src/fixtures/big.ts': 'export const x = 1;\n'.repeat(50),
      'src/lib/fmt.test.ts': `it('LIB-01 fmt', () => { expect('a').toBe('a') });\n`,
    });
    const out = checkTests(r.ctx('main'));
    expect(out.metrics?.filter((m) => m.name.startsWith('budget'))).toEqual([
      { name: 'budget.T1', value: 4, detail: '4 test / 1 product lines, budget 1' },
      { name: 'budget.T2', value: 'test-only', detail: '1 test lines, 0 product lines' },
    ]);
    expect(out.violations.filter((v) => v.rule === 'TEST-3').map((v) => v.message.split(':')[0])).toEqual(['T1 test lines are 4.00x product lines (budget 1)']);
  });
});

describe('DEPS-01 new dependencies block unless allowlisted', () => {
  it('lists added deps and only flags ones outside deps.allow', async () => {
    const r = repo({ 'package.json': JSON.stringify({ dependencies: { zod: '1' } }) }, { ...BASE_CONFIG, deps: { allow: ['ok-lib'] } }).commit();
    r.write({ 'package.json': JSON.stringify({ dependencies: { zod: '1', 'ok-lib': '1' }, devDependencies: { 'left-pad': '1' } }) });
    const out = await checkArch(r.ctx('main'));
    expect(out.violations.map((v) => [v.rule, v.fingerprint.length, v.message.split(' (')[0]])).toEqual([['ARCH-4', 12, 'adds dependency "left-pad"']]);
    expect(out.metrics).toEqual([{ name: 'deps.added', value: 2, detail: 'ok-lib, left-pad' }, { name: 'deps.registry', value: 'skipped', detail: 'offline: ok-lib, left-pad not checked against npm' }]);
  });

  it('still gates local and git deps on deps.allow but skips the npm registry for them', async () => {
    const r = repo({ 'package.json': JSON.stringify({ dependencies: {} }) }, { ...BASE_CONFIG, deps: { allow: ['@me/local'] } }).commit();
    r.write({ 'package.json': JSON.stringify({ devDependencies: { '@me/local': 'file:vendor/me-local-1.0.0.tgz', 'gh-lib': 'github:me/gh-lib#abc', 'left-pad': '1' } }) });
    const out = await checkArch(r.ctx('main'));
    expect(out.violations.map((v) => v.message.split(' (')[0])).toEqual(['adds dependency "gh-lib"', 'adds dependency "left-pad"']);
    expect(out.metrics).toEqual([{ name: 'deps.added', value: 3, detail: '@me/local, gh-lib, left-pad' }, { name: 'deps.registry', value: 'skipped', detail: 'offline: left-pad not checked against npm' }]);
  });
});
