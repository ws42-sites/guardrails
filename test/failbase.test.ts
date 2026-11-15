import { symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { checkFailsOnBase, titleMatcher } from '../src/checks/failbase.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const VITEST = `node ${JSON.stringify(join(process.cwd(), 'node_modules/vitest/vitest.mjs'))} run`;
const cfg = (items = {}) => ({ ...BASE_CONFIG, specs: { items: { ...BASE_CONFIG.specs.items, ...items } }, tests: { command: VITEST } });
const test = (title: string, body: string) => `import { expect, it } from 'vitest';\nimport { add } from './add';\nit('${title}', () => { ${body} });\n`;

function project(config = cfg()) {
  const r = repo({ '.gitignore': 'node_modules\n', 'src/core/add.ts': 'export const add = (a: number, b: number) => a - b;\n' }, config);
  symlinkSync(join(process.cwd(), 'node_modules'), join(r.dir, 'node_modules'));
  return r.commit();
}
const outcome = (r: ReturnType<typeof project>, labels: string[] = []) => {
  const out = checkFailsOnBase(r.ctx('main', labels));
  return [out.violations.map((v) => v.message.split(' already')[0].split(' does not')[0]), out.metrics?.[0]?.value];
};

describe('FAILBASE-01 new and changed tests must fail on base and pass on head', () => {
  it('passes a test that fails before the fix and passes after it', () => {
    const r = project();
    r.write({ 'src/core/add.ts': 'export const add = (a: number, b: number) => a + b;\n', 'src/core/add.test.ts': test('CORE-01 adds', 'expect(add(2, 2)).toBe(4)') });
    expect(outcome(r)).toEqual([[], '1/1']);
  });

  it('blocks a test that already passes on base unless the spec is a regression guard or the PR is labelled', () => {
    const r = project();
    r.write({ 'src/core/add.ts': 'export const add = (a: number, b: number) => a + b;\n', 'src/core/add.test.ts': test('CORE-01 zero', 'expect(add(0, 0)).toBe(0)') });
    expect(outcome(r)).toEqual([['"CORE-01 zero"'], '0/1']);
    expect(outcome(r, ['allow-characterization'])).toEqual([[], '0/1']);
    const guard = project(cfg({ 'CORE-01': { tier: 'T1', behaviour: 'core adds numbers', regressionGuard: true } }));
    guard.write({ 'src/core/add.test.ts': test('CORE-01 zero', 'expect(add(0, 0)).toBe(0)') });
    expect(outcome(guard)).toEqual([[], '0/1']);
  });

  it('blocks a new test that fails on head and skips unchanged tests', () => {
    const r = project();
    r.write({ 'src/core/add.test.ts': test('CORE-01 adds', 'expect(add(2, 2)).toBe(4)') });
    expect(outcome(r)).toEqual([['"CORE-01 adds"'], '0/1']);
    r.write({ 'src/core/add.test.ts': test('CORE-01 subtracts', 'expect(add(2, 2)).toBe(0)') }).commit();
    expect(checkFailsOnBase(r.ctx('HEAD')).metrics).toEqual([{ name: 'failsOnBase', value: 'skipped', detail: 'no new or changed tests' }]);
  });

  it.each([
    ['CORE-01 adds %i and %i', 'CORE-01 adds 1 and 2', true],
    ['CORE-01 adds $a', 'CORE-01 adds 5', true],
    ['CORE-01 adds (x)', 'CORE-01 adds (x)', true],
    ['CORE-01 adds', 'CORE-01 adds more', false],
  ])('matches it.each title %s against %s', (title, reported, matches) => {
    expect(titleMatcher(title).test(reported)).toBe(matches);
  });
});
