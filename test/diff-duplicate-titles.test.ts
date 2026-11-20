import { expect, it } from 'vitest';
import { checkTests } from '../src/checks/tests.js';
import { repo } from './helpers/repo.js';

const shared = (a: string, b: string) => `it.each([1])('CORE-01 row', () => { ${a} });\nit.each([2])('CORE-01 row', () => { ${b} });\n`;
const one = 'expect(1).toBe(1)';
const two = 'expect(1).toBe(1); expect(2).toBe(2)';

it.each([
  { head: shared(one, two), expected: [] },
  { head: shared(two, one), expected: [{ line: 2, message: '"CORE-01 row" lost 1 expect() call(s). Restore them.' }] },
])('DIFF-02 pairs base and head tests that share a title by position: %#', ({ head, expected }) => {
  const r = repo({ 'src/core/a.test.ts': shared(one, two) }).commit();
  r.write({ 'src/core/a.test.ts': head });
  const found = checkTests(r.ctx('main')).violations.filter((v) => v.rule === 'TEST-7').map((v) => ({ line: v.line, message: v.message }));
  expect(found).toEqual(expected);
});
