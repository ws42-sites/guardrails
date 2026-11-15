import { existsSync } from 'node:fs';
import { join } from 'node:path';
import type { GuardrailsConfig } from './config.js';
import { git, isE2eFile, isTestFile, listFiles, matchesAny, mergeBase, readText } from './util.js';

export interface Affected { all: boolean; reason?: string; changed: string[]; tests: string[]; e2e: string[] }

/** Files changed vs merge-base(base, HEAD), including uncommitted and untracked ones. */
export function changedFiles(cwd: string, base: string): string[] {
  const mb = mergeBase(cwd, base);
  const out = new Set(git(cwd, ['diff', '--name-only', '--no-renames', mb]).split('\n').filter(Boolean));
  for (const f of git(cwd, ['ls-files', '--others', '--exclude-standard']).split('\n').filter(Boolean)) out.add(f);
  return [...out];
}

/**
 * Tests affected by a change: a test is affected when it, or anything it imports (transitively, via
 * dependency-cruiser), changed. Changes to global files (lockfile, test config) select everything.
 * Tests whose source names a changed file (read by path) are selected too.
 * E2E files are selected by the same graph plus journeys whose routes or spec paths changed.
 */
export async function affectedTests(cwd: string, cfg: GuardrailsConfig, base: string): Promise<Affected> {
  const changed = changedFiles(cwd, base);
  const files = listFiles(cwd);
  const tests = files.filter((f) => isTestFile(cfg, f) && (!cfg.ci.tests.length || matchesAny(f, cfg.ci.tests)));
  const e2eFiles = files.filter((f) => isE2eFile(cfg, f));
  const global = changed.find((f) => matchesAny(f, cfg.ci.globalFiles));
  if (global) return { all: true, reason: `${global} changed`, changed, tests, e2e: e2eFiles };
  const roots = [...tests, ...e2eFiles];
  if (!roots.length || !changed.length) return { all: false, changed, tests: [], e2e: [] };

  const { cruise } = await import('dependency-cruiser');
  const tsConfig = cfg.architecture.tsConfig ?? (existsSync(join(cwd, 'tsconfig.json')) ? 'tsconfig.json' : undefined);
  const result = await cruise(roots, {
    baseDir: cwd, doNotFollow: { path: 'node_modules' }, tsPreCompilationDeps: true,
    ...(tsConfig ? { tsConfig: { fileName: join(cwd, tsConfig) } } : {}),
  } as never);
  const modules = (result.output as unknown as { modules: { source: string; dependencies: { resolved: string }[] }[] }).modules;
  const deps = new Map(modules.map((m) => [m.source, m.dependencies.map((d) => d.resolved)]));
  const hit = new Set(changed);
  const memo = new Map<string, boolean>();
  const reaches = (f: string, seen = new Set<string>()): boolean => {
    if (memo.has(f)) return memo.get(f) as boolean;
    if (hit.has(f)) return true;
    if (seen.has(f)) return false;
    seen.add(f);
    const r = (deps.get(f) ?? []).some((d) => reaches(d, seen));
    memo.set(f, r);
    return r;
  };
  // Tests that read a changed file by path (fixtures, built assets, HTML) are affected too.
  const names = changed.map((c) => c.split('/').pop() as string).filter((n) => n.length >= 6);
  const mentions = (f: string) => { const t = readText(cwd, f); return names.some((n) => t.includes(n)); };
  const journeyIds = Object.entries(cfg.journeys.items)
    .filter(([, j]) => changed.some((c) => matchesAny(c, j.routes) || j.specs.some((s) => matchesAny(c, cfg.specs.items[s]?.paths ?? []))))
    .map(([id]) => id);
  const e2e = e2eFiles.filter((f) => reaches(f) || journeyIds.some((id) => readText(cwd, f).includes(id)));
  return { all: false, changed, tests: tests.filter((t) => reaches(t) || mentions(t)), e2e };
}
