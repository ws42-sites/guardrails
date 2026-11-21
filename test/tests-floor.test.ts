import { expect, it } from 'vitest';
import { checkTests, compareConfigs, indexTests, weakness } from '../src/checks/tests.js';
import { inventory } from '../src/inventory.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

type Json = Parameters<typeof compareConfigs>[0];
const changes: [Json, Json, string, string[]][] = [
  [true, false, 'enabled', ['enabled: disabled']],
  [false, false, 'enabled', []],
  [true, false, 'feature.enabledSuffix', []],
  [true, false, 'feature.specTests', ['feature.specTests: disabled']],
  [false, true, 'specs.items.X.regressionGuard', ['specs.items.X.regressionGuard: set (tests may pass on base)']],
  [true, true, 'specs.items.X.regressionGuard', []],
  [false, false, 'specs.items.X.regressionGuard', []],
  [1, 2, 'docs.maxLines.guide', ['docs.maxLines.guide: 1 -> 2']],
  [1, 2, 'other.maxFileLinesSuffix', []],
  [2, 1, 'other.minimum', ['other.minimum: 2 -> 1']],
  [1, 1, 'other.minimum', []],
  ['warn', 'warn', 'rules.DOC-1', []],
  ['off', 'block', 'rules.DOC-1', []],
  ['unknown', 'warn', 'rules.X', ['rules.X: unknown -> warn']],
  ['off', 'unknown', 'rules.X', []],
  ['T2', 'T1', 'specs.items.X.tier', []],
  ['T1', 'T2', 'other.tier', []],
  ['T1', 'T2', 'specs.items.X.title', []],
  [[], ['x'], 'other.allow', []],
  [['a'], ['a', 'b'], 'deps.allow', ['deps.allow: added "b"']],
  [['a'], [], 'tests.globs', ['tests.globs: removed "a"']],
  [{ required: 1 }, {}, 'tests', ['tests.required: removed']],
  [{ '$schema': 'a', rules: { X: 'off' }, specs: { items: { X: {} } }, journeys: { items: { J: { approved: true } } } }, { rules: {}, specs: { items: {} }, journeys: { items: {} } }, '', []],
  [{}, { runbook: { enabled: false }, testSuite: { enabled: false } }, '', []],
  [{}, { other: { runbook: { enabled: false }, testSuite: { enabled: false } } }, '', ['other.runbook.enabled: disabled', 'other.testSuite.enabled: disabled']],
  [{}, { journeys: { items: { J: { enabled: false } } } }, '', []],
  [{}, { deps: { allow: ['new'] }, duplication: { enabled: false }, optional: null }, '', ['deps.allow: added "new"', 'duplication.enabled: disabled']],
  [{}, { rules: { 'DOC-1': 'warn', 'TEST-7': 'block', UNKNOWN: 'warn' } }, '', ['rules.UNKNOWN: set to warn']],
  [null, {}, '', []],
  [{}, null, '', []],
  [1, '1', '', []],
  ['1', 1, '', []],
  [true, 'false', 'enabled', []],
  [{}, [], '', []],
  [[], {}, '', []],
];

it.each(changes)('DIFF-03 compares exact weakening semantics %#', (before, after, path, expected) => {
  expect(compareConfigs(before, after, path)).toEqual(expected);
});

it.each([
  ['expect(fn).not.toHaveBeenCalled()', 'only asserts that a mock was called. Also assert the result the caller sees.'],
  ['expect(fn).toHaveBeenCalledOnce()', 'only asserts that a mock was called. Also assert the result the caller sees.'],
  ['expect(value).not.toBeNull()', 'only tautological assertions (not.toBeNull). Assert the exact value.'],
  ['expect(value).toBeTruthy(); expect(value).toBeTruthy()', 'only tautological assertions (toBeTruthy). Assert the exact value.'],
  ['expect(1).toBe(1)', 'only tautological assertions (toBe). Assert the exact value.'],
  ['expect(value).toBeTruthy(); expect(value).toBe(2)', null],
  ['expect(value).toHaveBeenCalledCustom()', null],
])('WEAK-01 reports exact weakness for %s', (body, expected) => {
  const test = inventory(`it('CORE-01 sample', () => { ${body} });`, '^[A-Z]+-[0-9]+$').tests[0];
  expect(weakness(test)).toBe(expected);
});

it('SPEC-01 indexes parse failures without losing the filename', () => {
  const r = repo({ 'src/a.test.ts': 'it(' });
  const index = indexTests(r.ctx());
  expect(Object.keys(index.parseErrors)).toEqual(['src/a.test.ts']);
  expect(index.byFile).toEqual({ 'src/a.test.ts': [] });
});

it('SIZE-01 permits exact file and spec-count boundaries', () => {
  const r = repo({ 'src/a.test.ts': "it('CORE-01 adds', () => { expect(add()).toBe(1) });" }, { ...BASE_CONFIG, specs: { ...BASE_CONFIG.specs, maxTestsPerId: 1 }, tests: { maxFileLines: 1 } });
  expect(checkTests(r.ctx()).violations.filter((v) => v.rule === 'TEST-4')).toEqual([]);
});

it('SPEC-02 reports untested tiers with exact messages and metrics', () => {
  const r = repo();
  const out = checkTests(r.ctx());
  expect(out.violations.map(({ rule, file, line, message, maxMode }) => ({ rule, file, line, message, maxMode }))).toEqual([
    { rule: 'TEST-2', file: 'guardrails.config.json', line: undefined, message: 'CORE-01 (T1) "core adds numbers" has no test.', maxMode: undefined },
    { rule: 'TEST-2', file: 'guardrails.config.json', line: undefined, message: 'LIB-01 (T2) "lib formats text" has no test.', maxMode: 'warn' },
  ]);
  expect(out.metrics).toEqual([{ name: 'tests', value: 0 }, { name: 'specs', value: 2, detail: '0 with tests' }]);
});

