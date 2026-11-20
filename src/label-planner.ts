import { existsSync } from 'node:fs';
import { isAbsolute, join, relative } from 'node:path';
import { checkTests } from './checks/tests.js';
import { checkSuite } from './checks/suite.js';
import { checkJourneys } from './checks/journeys.js';
import { readFlags } from './flags.js';
import { inventory } from './inventory.js';
import type { CheckContext } from './types.js';
import { diffAgainst, isProductFile, isTestFile, listFiles, matchesAny, mergeBase, readText, showAt } from './util.js';

export interface LabelPlanEntry {
  label: string;
  reason: string;
  file: string;
  line?: number;
  status: 'needed' | 'already present';
}

async function importGraph(ctx: CheckContext, files: string[]): Promise<Map<string, string[]>> {
  const { cruise } = await import('dependency-cruiser');
  const tsConfig = ctx.config.architecture.tsConfig ?? (existsSync(join(ctx.cwd, 'tsconfig.json')) ? 'tsconfig.json' : undefined);
  const result = await cruise(files.map((f) => join(ctx.cwd, f)), {
    baseDir: ctx.cwd, doNotFollow: { path: 'node_modules' }, tsPreCompilationDeps: true,
    ...(tsConfig ? { tsConfig: { fileName: join(ctx.cwd, tsConfig) } } : {}),
  });
  const modules = (result.output as { modules: { source: string; dependencies: { resolved: string }[] }[] }).modules;
  const normalise = (f: string) => (isAbsolute(f) ? relative(ctx.cwd, f) : f).replaceAll('\\', '/');
  return new Map(modules.map((m) => [normalise(m.source), m.dependencies.map((d) => normalise(d.resolved))]));
}

export async function planLabels(ctx: CheckContext): Promise<LabelPlanEntry[]> {
  const plan: LabelPlanEntry[] = [];
  const cfg = ctx.config;
  const add = (label: string, reason: string, file: string, line?: number) => {
    plan.push({ label, reason, file, ...(line === undefined ? {} : { line }), status: ctx.labels.includes(label) ? 'already present' : 'needed' });
  };
  const unapproved = { ...ctx, labels: [] };
  if (ctx.base) {
    const files = listFiles(ctx.cwd);
    const diff = diffAgainst(ctx.cwd, ctx.base);
    const mb = mergeBase(ctx.cwd, ctx.base);
    const changedProduct = new Set(Object.keys(diff).filter((f) => isProductFile(cfg, f)));
    const tests = Object.keys(diff).filter((f) => isTestFile(cfg, f) && diff[f].status !== 'deleted');
    const graph = tests.length ? await importGraph(ctx, tests) : new Map<string, string[]>();
    const reachesChange = (file: string, seen = new Set<string>()): boolean => {
      if (changedProduct.has(file)) return true;
      if (seen.has(file)) return false;
      seen.add(file);
      return (graph.get(file) ?? []).some((dependency) => reachesChange(dependency, seen));
    };
    for (const file of tests) {
      const before = inventory(showAt(ctx.cwd, mb, file) ?? '', cfg.specs.idPattern, file).tests;
      const head = inventory(readText(ctx.cwd, file), cfg.specs.idPattern, file);
      if (head.parseError) throw new Error(`${file}: ${head.parseError}`);
      for (const test of head.tests) {
        const spec = test.specId ? cfg.specs.items[test.specId] : undefined;
        if (!spec || spec.regressionGuard || before.some((t) => t.title === test.title && t.hash === test.hash)) continue;
        if ([...changedProduct].some((f) => matchesAny(f, spec.paths ?? [])) || reachesChange(file)) continue;
        add(cfg.labels.characterization, `Existing behaviour: ${test.title}; matching product files did not change.`, file, test.line);
      }
    }
    for (const v of checkTests(unapproved, files).violations) {
      if (v.rule === 'TEST-7' && v.message.startsWith('config loosened:')) add(cfg.labels.configApproved, v.message, v.file, v.line);
      if (v.rule === 'TEST-7' && v.message.startsWith('deleted base test ')) add(cfg.tests.testsRemovedLabel, v.message, v.file, v.line);
      if (v.rule === 'BASE-1') add(cfg.labels.rebaseline, v.message, v.file, v.line);
    }
    for (const v of checkSuite(unapproved, files).violations)
      if (v.rule === 'SUITE-6' && v.message.startsWith('test-suite baseline grew')) add(cfg.labels.rebaseline, v.message, v.file, v.line);
    for (const v of checkJourneys(unapproved, files).violations)
      if (v.rule === 'JRN-3') add(cfg.labels.journeysApproved, v.message, v.file, v.line);
  }
  for (const flag of readFlags(ctx.cwd, cfg.flagsFile).flags)
    add(cfg.labels.flagsResolved, `Open flag ${flag.id} on ${flag.specId}: ${flag.reason}`, flag.file || cfg.flagsFile);
  return plan;
}

export function toMarkdown(plan: LabelPlanEntry[]): string {
  const cell = (value: string) => value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('|', '&#124;').replace(/\r?\n/g, ' ');
  const rows = plan.map((entry) => `| ${cell(entry.label)} (${entry.status}) | ${cell(entry.reason)} | ${cell(entry.file)}${entry.line === undefined ? '' : `:${entry.line}`} |`);
  return ['| Label | Why | File:line |', '| --- | --- | --- |', ...rows,
    ...(plan.length ? [] : ['No approval labels needed.']), '',
    'A code owner adds these labels once; Guardrails never adds labels.', ''].join('\n');
}
