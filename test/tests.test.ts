import { describe, expect, it } from 'vitest';
import { checkTests, weakness } from '../src/checks/tests.js';
import { inventory } from '../src/inventory.js';
import { repo, rules, BASE_CONFIG } from './helpers/repo.js';

const ID = '^[A-Z][A-Z0-9]{1,11}-[0-9]{2,4}$';

describe('SPEC-01 test titles carry a known spec ID', () => {
  it('reads IDs from the test title or the nearest describe, counting it.each once', () => {
    const inv = inventory(`
      describe('CORE-01 adds', () => { it('works', () => { expect(1).toBe(1) }) });
      it('LIB-01 formats', () => {});
      it.each([[1], [2]])('CORE-01 table %i', (n) => { expect(n).toBeGreaterThan(0) });
      it('[core-1] lower', () => {});
      it('no id', () => {});`, ID);
    expect(inv.tests.map((t) => [t.specId, t.malformedId, t.each])).toEqual([
      ['CORE-01', null, false], ['LIB-01', null, false], ['CORE-01', null, true], [null, 'core-1', false], [null, null, false],
    ]);
  });

  it('flags missing, malformed and unknown IDs', () => {
    const r = repo({ 'src/a.test.ts': `it('CORE-01 ok', () => { expect(1).toBe(1) });\nit('nothing', () => { expect(1).toBe(1) });\nit('ZZZ-01 x', () => { expect(1).toBe(1) });\nit('core-1 x', () => { expect(1).toBe(1) });` });
    const msgs = checkTests(r.ctx()).violations.filter((v) => v.rule === 'TEST-1').map((v) => v.message.split('.')[0]);
    expect(msgs).toEqual(['"nothing" has no spec ID', '"ZZZ-01 x" uses ZZZ-01, which is not in specs', '"core-1 x": "core-1" does not match ^[A-Z][A-Z0-9]{1,11}-[0-9]{2,4}$']);
  });
});

describe('SPEC-02 high-tier specs need a test', () => {
  it('blocks on an untested T1 spec and caps T2 gaps at warn', () => {
    const v = checkTests(repo().ctx()).violations.filter((x) => x.rule === 'TEST-2');
    expect(v.map((x) => [x.message.slice(0, 7), x.maxMode ?? 'block'])).toEqual([['CORE-01', 'block'], ['LIB-01 ', 'warn']]);
  });
});

describe('SIZE-01 test files and per-ID counts are capped', () => {
  it('warns over maxFileLines and maxTestsPerId', () => {
    const cfg = { ...BASE_CONFIG, specs: { ...BASE_CONFIG.specs, maxTestsPerId: 2 }, tests: { maxFileLines: 3 } };
    const body = ['a', 'b', 'c'].map((x) => `it('CORE-01 ${x}', () => { expect(add('${x}')).toBe(1) });`).join('\n') + '\n// end';
    const v = checkTests(repo({ 'src/a.test.ts': body }, cfg).ctx()).violations;
    expect(rules(v)).toEqual(['TEST-2', 'TEST-4', 'TEST-4']);
    expect(v.filter((x) => x.rule === 'TEST-4').map((x) => x.message)).toEqual([
      '4 lines (cap 3). Split by behaviour or use it.each.',
      'CORE-01 has 3 tests (cap 2). That is padding; merge into it.each.',
    ]);
  });
});

describe('WEAK-01 weak-assertion heuristics warn', () => {
  const t = (src: string) => weakness(inventory(src, ID).tests[0]);
  it.each([
    [`it('A-01', () => { expect(fn).toHaveBeenCalledTimes(1) })`, /only asserts that a mock was called/],
    [`it('A-01', () => { expect(x).toBeDefined(); expect(y).toBeTruthy() })`, /only tautological assertions \(toBeDefined, toBeTruthy\)/],
    [`it('A-01', () => { expect(true).toBe(true) })`, /only tautological/],
    [`it('A-01', () => { expect(() => f()).not.toThrow() })`, /only tautological/],
    [`it('A-01', () => { f() })`, /no expect\(\) calls/],
  ])('flags %s', (src, message) => {
    expect(t(src)).toMatch(message);
  });

  it('accepts a mock assertion paired with a result assertion', () => {
    expect(t(`it('A-01', () => { expect(fn).toHaveBeenCalled(); expect(out).toEqual({ ok: true }) })`)).toBeNull();
  });
});
