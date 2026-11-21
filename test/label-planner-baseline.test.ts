import { config, fixture, flagFile, labelRepo, labels, testSource } from './helpers/label-planner.js';
import { expect, it, vi } from 'vitest';
import { join } from 'node:path';
import { cruise } from 'dependency-cruiser';
import { planLabels } from '../src/label-planner.js';

it.each([-1, 0, 1, 2])('LPLAN-01 reports suite baseline growth to %s', async (count) => {
  const baseline = (n: number) => JSON.stringify({ version: 1, byKind: { focus: n }, totals: {}, counts: {}, oversize: {} });
  const r = labelRepo({ '.guardrails/test-suite-baseline.json': baseline(1) }, { ...config, testSuite: { enabled: true, maxLines: 1 } });
  r.write({ '.guardrails/test-suite-baseline.json': baseline(count) });
  if (count === -1) {
    r.remove('.guardrails/test-suite-baseline.json');
    r.write({ 'test/oversize.test.ts': 'export const a = 1;\nexport const b = 2;\n' });
  }
  const plan = await planLabels(r.ctx('HEAD'));
  expect(plan).toEqual(count > 1 ? [{
    label: labels.rebaseline, file: '.guardrails/test-suite-baseline.json', status: 'needed',
    reason: 'test-suite baseline grew (focus 1 -> 2). Fix the new findings instead, or have a code owner add the "owner:baseline" label, which approves letting the test-suite baseline grow.',
  }] : []);
  for (const entry of plan) expect(Object.hasOwn(entry, 'line')).toBe(false);
});

it.each([false, true])('LPLAN-02 handles flags without a product location and empty plans %s', async (flag) => {
  const r = labelRepo(flag ? { '.guardrails/flags.json': flagFile.replace('src/core/add.ts', '') } : {});
  r.write({ 'test/new.test.ts': testSource() });
  expect(await planLabels(r.ctx())).toEqual(flag ? [{
    label: labels.flagsResolved, file: '.guardrails/flags.json', status: 'needed',
    reason: 'Open flag F1 on CORE-01: Conflicting requirement',
  }] : []);
});

it.each(['explicit', 'automatic', 'absent'])('LPLAN-01 resolves absolute graph paths with %s TypeScript configuration', async (mode) => {
  const r = labelRepo(mode === 'automatic' ? { 'tsconfig.json': '{}' } : {}, {
    ...config, specs: { items: { 'CORE-01': { tier: 'T1', behaviour: 'core adds numbers' } } },
    ...(mode === 'explicit' ? { architecture: { tsConfig: 'tsconfig.graph.json' } } : {}),
  });
  r.write({ 'test/add.test.ts': testSource(), 'src/core/add.ts': 'export const add = () => 2;' });
  fixture.modules = [
    { source: join(r.dir, 'test/add.test.ts'), dependencies: [{ resolved: join(r.dir, 'src/core/add.ts') }] },
  ];
  vi.mocked(cruise).mockClear();
  expect(await planLabels(r.ctx('HEAD'))).toEqual([]);
  expect(vi.mocked(cruise).mock.calls).toEqual([[[join(r.dir, 'test/add.test.ts')], {
    baseDir: r.dir, doNotFollow: { path: 'node_modules' }, tsPreCompilationDeps: true,
    ...(mode === 'absent' ? {} : { tsConfig: { fileName: join(r.dir, mode === 'explicit' ? 'tsconfig.graph.json' : 'tsconfig.json') } }),
  }]]);
});
