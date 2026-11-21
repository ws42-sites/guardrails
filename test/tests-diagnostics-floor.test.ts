import { expect, it } from 'vitest';
import { checkTests } from '../src/checks/tests.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const details = (r: ReturnType<typeof repo>, rule: string, base: string | null = null) => checkTests(r.ctx(base)).violations.filter((v) => v.rule === rule).map(({ rule, file, line, message, maxMode }) => ({ rule, file, line, message, maxMode }));

it('SPEC-01 preserves exact malformed, missing and unknown diagnostics', () => {
  const r = repo({ 'src/a.test.ts': "it('nothing', () => { expect(x).toBe(2) });\nit('NOPE-01 bad', () => { expect(x).toBe(2) });\nit('core-1 bad', () => { expect(x).toBe(2) });" });
  expect(details(r, 'TEST-1')).toEqual([
    { rule: 'TEST-1', file: 'src/a.test.ts', line: 1, message: '"nothing" has no spec ID. Start the title (or its describe) with an ID from guardrails.config.json specs.items, e.g. "REVIEW-03 ...".', maxMode: 'warn' },
    { rule: 'TEST-1', file: 'src/a.test.ts', line: 2, message: '"NOPE-01 bad" uses NOPE-01, which is not in specs.items. Add the behaviour to the spec block first (a code owner reviews it).', maxMode: undefined },
    { rule: 'TEST-1', file: 'src/a.test.ts', line: 3, message: '"core-1 bad": "core-1" does not match ^[A-Z][A-Z0-9]{1,11}-[0-9]{2,4}$.', maxMode: undefined },
  ]);
});

it('WEAK-01 includes the weak test title and line in the finding', () => {
  const r = repo({ 'src/a.test.ts': "\nit('CORE-01 weak', () => { expect(x).toBeTruthy() });" });
  expect(details(r, 'TEST-5W')).toEqual([{ rule: 'TEST-5W', file: 'src/a.test.ts', line: 2, message: '"CORE-01 weak": only tautological assertions (toBeTruthy). Assert the exact value.', maxMode: undefined }]);
});

it('SIZE-01 reports the first test file when a spec crosses the cap', () => {
  const r = repo({ 'src/a.test.ts': "it('CORE-01 first', () => { expect(x).toBe(2) });\nit('CORE-01 second', () => { expect(x).toBe(3) });" }, { ...BASE_CONFIG, specs: { ...BASE_CONFIG.specs, maxTestsPerId: 1 } });
  expect(details(r, 'TEST-4')).toEqual([{ rule: 'TEST-4', file: 'src/a.test.ts', line: undefined, message: 'CORE-01 has 2 tests (cap 1). That is padding; merge into it.each.', maxMode: undefined }]);
});

it.each([
  ['it(\'LIB-01 table\', () => { expect(value).toBe(2) });', 'T2', 0.6, [{ rule: 'TEST-3', file: '(diff)', line: undefined, message: 'T2 test lines are 1.00x product lines (budget 0.6): 1 test vs 1 product.', maxMode: undefined }]],
  ['it(\'CORE-01 table\', () => { expect(value).toBe(2) });\nit(\'LIB-01 table\', () => { expect(value).toBe(2) });', 'T1', 1, [{ rule: 'TEST-3', file: '(diff)', line: undefined, message: 'T1 test lines are 2.00x product lines (budget 1): 2 test vs 1 product.', maxMode: undefined }]],
  ['it(\'CORE-01 table\', () => { expect(value).toBe(2) });', 'T1', 1, []],
])('BUDGET-01 assigns mixed-spec test files to their highest tier %#', (source, tier, budget, findings) => {
  const r = repo().commit();
  const file = tier === 'T1' ? 'src/core/a.ts' : 'src/lib/a.ts';
  r.write({ [file]: '\n // intent\n/* block */\n * continuation\nexport const value = 2;\n', 'src/a.test.ts': source });
  const out = checkTests(r.ctx('main'));
  const count = source.split('\n').length;
  expect(out.metrics?.filter((m) => m.name.startsWith('budget'))).toEqual([{ name: `budget.${tier}`, value: count, detail: `${count} test / 1 product lines, budget ${budget}` }]);
  expect(details(r, 'TEST-3', 'main')).toEqual(findings);
});

it('DIFF-01 preserves deletion metrics and complete human guidance', () => {
  const r = repo({ 'src/a.test.ts': "it('CORE-01 lost', () => { expect(x).toBe(2) });" }).commit();
  r.remove('src/a.test.ts');
  const out = checkTests(r.ctx('main'));
  expect(details(r, 'TEST-7', 'main')).toEqual([{ rule: 'TEST-7', file: 'src/a.test.ts', line: undefined, message: 'deleted base test "CORE-01 lost". Deleting base tests needs a code owner to add the "guardrails:tests-removed-approved" label, which approves removing tests.', maxMode: undefined }]);
  expect(details(r, 'TEST-10', 'main')).toEqual([{ rule: 'TEST-10', file: '(diff)', line: undefined, message: 'test count dropped by 1 (1 on base, 0 now).', maxMode: undefined }]);
  expect(out.metrics?.filter((m) => m.name.startsWith('tests.'))).toEqual([{ name: 'tests.vsBase', value: -1, detail: '1 on base, 0 now' }, { name: 'tests.deletedFromBase', value: 1, detail: 'src/a.test.ts: CORE-01 lost' }]);
});
