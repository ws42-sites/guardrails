import { config, fixture, flagFile, labelRepo, labels, testSource, title } from './helpers/label-plan.js';
import { expect, it } from 'vitest';
import { planLabels } from '../src/label-plan.js';

it.each([
  { name: 'new', old: '', next: testSource(), product: false, guard: false, expected: true },
  { name: 'changed', old: testSource(2), next: testSource(), product: false, guard: false, expected: true },
  { name: 'unchanged', old: testSource(), next: testSource(), product: false, guard: false, expected: false },
  { name: 'line shifted', old: testSource(), next: '\r\n' + testSource(), product: false, guard: false, expected: false },
  { name: 'product changed', old: '', next: testSource(), product: true, guard: false, expected: false },
  { name: 'regression guard', old: '', next: testSource(), product: false, guard: true, expected: false },
  { name: 'unknown spec', old: '', next: testSource().replace('CORE-01', 'OTHER-01'), product: false, guard: false, expected: false },
  { name: 'untagged', old: '', next: testSource().replace('CORE-01 ', ''), product: false, guard: false, expected: false },
])('LPLAN-01 classifies $name tests using spec paths', async ({ old, next, product, guard, expected }) => {
  const cfg = { ...config, specs: { items: { 'CORE-01': { ...config.specs.items['CORE-01'], regressionGuard: guard } } } };
  const r = labelRepo({ 'test/add.test.ts': old }, cfg);
  r.write({ 'test/add.test.ts': next });
  if (product) r.write({ 'src/core/add.ts': 'export const add = () => 2;\n' });
  expect(await planLabels(r.ctx('HEAD'))).toEqual(expected ? [{
    label: labels.characterization, reason: `Existing behaviour: ${title}; matching product files did not change.`,
    file: 'test/add.test.ts', line: 1, status: 'needed',
  }] : []);
});

it.each([
  ['unchanged imports', 'src/other.ts', true],
  ['direct product', 'src/core/add.ts', false],
  ['transitive product', 'src/core/value.ts', false],
  ['helper only', 'test/helper.ts', true],
])('LPLAN-01 follows the import graph for %s', async (_, changed, expected) => {
  const r = labelRepo({
    'src/core/add.ts': "export { add } from './value.js';\n",
    'src/core/value.ts': "import './add.js'; export const add = () => 1;\n",
    'test/helper.ts': "export { add } from '../src/core/add.js';\n",
  }, { ...config, specs: { items: { 'CORE-01': { tier: 'T1', behaviour: 'adds numbers' } } } });
  fixture.modules = [
    { source: 'test/add.test.ts', dependencies: [{ resolved: 'test/helper.ts' }] },
    { source: 'test/helper.ts', dependencies: [{ resolved: 'src/core/add.ts' }] },
    { source: 'src/core/add.ts', dependencies: [{ resolved: 'src/core/value.ts' }] },
    { source: 'src/core/value.ts', dependencies: [{ resolved: 'src/core/add.ts' }] },
  ];
  r.write({ 'test/add.test.ts': "import { add } from './helper.js';\n" + testSource(), [changed]: 'export const add = () => 2;\n' });
  expect(await planLabels(r.ctx('HEAD'))).toEqual(expected ? [{
    label: labels.characterization, reason: `Existing behaviour: ${title}; matching product files did not change.`,
    file: 'test/add.test.ts', line: 2, status: 'needed',
  }] : []);
});

it.each(['deleted', 'moved', 'weakened', 'strengthened'] as const)('LPLAN-01 reuses TEST-7 for %s tests', async (change) => {
  const r = labelRepo({ 'test/old.test.ts': testSource() });
  if (change === 'deleted' || change === 'moved') r.remove('test/old.test.ts');
  if (change === 'moved') r.write({ 'test/new.test.ts': testSource(), 'src/core/add.ts': 'export const add = () => 2;' });
  if (change === 'weakened') r.write({ 'test/old.test.ts': testSource().replace('toBe', 'toBeTruthy'), 'src/core/add.ts': 'export const add = () => 2;' });
  if (change === 'strengthened') r.write({ 'test/old.test.ts': testSource().replace('toBe', 'toStrictEqual'), 'src/core/add.ts': 'export const add = () => 2;' });
  expect(await planLabels(r.ctx('HEAD'))).toEqual(change === 'deleted' ? [{
    label: 'owner:removed', file: 'test/old.test.ts', status: 'needed',
    reason: 'deleted base test "CORE-01 adds". Deleting base tests needs a code owner to add the "owner:removed" label, which approves removing tests.',
  }] : []);
});

it.each([60, 70, 80])('LPLAN-01 reuses config weakening at threshold %s', async (threshold) => {
  const r = labelRepo({}, { ...config, mutation: { break: 70 } });
  r.write({ 'guardrails.config.json': JSON.stringify({ ...config, mutation: { break: threshold } }) });
  expect(await planLabels(r.ctx('HEAD', [labels.configApproved]))).toEqual(threshold < 70 ? [{
    label: labels.configApproved, file: 'guardrails.config.json', status: 'already present',
    reason: 'config loosened: mutation.break: 70 -> 60. Lowering a threshold counts as weakening; a code owner must add the "owner:config" label, which approves loosening guardrails.config.json.',
  }] : []);
});

it.each([true, false])('LPLAN-01 reports baseline entry growth %s', async (grow) => {
  const r = labelRepo({ '.guardrails/baseline.json': JSON.stringify({ entries: [{ rule: 'DOC-1', fingerprint: 'old' }] }) });
  r.write({ '.guardrails/baseline.json': JSON.stringify({ entries: grow ? [{ rule: 'DOC-1', fingerprint: 'new' }] : [] }) });
  expect(await planLabels(r.ctx('HEAD'))).toEqual(grow ? [{
    label: labels.rebaseline, file: '.guardrails/baseline.json', status: 'needed',
    reason: 'baseline grew by 1 entry. Fix the new violations instead, or have a code owner add the "owner:baseline" label, which approves letting the baseline grow.',
  }] : []);
});

it('LPLAN-01 reports journey ownership changes with their approval label', async () => {
  const r = labelRepo();
  r.write({ 'guardrails.config.json': JSON.stringify({ ...config, journeys: { items: {
    'CUJ-ADD': { title: 'Add two numbers', priority: 'P2', steps: ['Add numbers'], specs: ['CORE-01'], routes: [] },
  } } }) });
  expect(await planLabels(r.ctx('HEAD'))).toEqual([{
    label: labels.journeysApproved, file: 'guardrails.config.json', status: 'needed',
    reason: 'journey CUJ-ADD added in this PR. Journeys are human-owned: only base-trusted producers may add valid run journeys or regenerate unapproved non-client journeys; a code owner adds the "owner:journeys" label, which approves these journey changes.',
  }]);
});

it.each([true, false])('LPLAN-02 reports open flags with existing approval %s', async (present) => {
  const r = labelRepo({ '.guardrails/flags.json': flagFile });
  const ctx = r.ctx(null, present ? [labels.flagsResolved] : []);
  const original = structuredClone(ctx);
  expect(await planLabels(ctx)).toEqual([{
    label: labels.flagsResolved, file: 'src/core/add.ts', status: present ? 'already present' : 'needed',
    reason: 'Open flag F1 on CORE-01: Conflicting requirement',
  }]);
  expect(ctx).toEqual(original);
});
