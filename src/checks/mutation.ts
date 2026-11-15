import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { diffAgainst, isProductFile, listFiles, matchesAny, tierOf, violation } from '../util.js';
import { runCommand } from './run.js';

interface StrykerMutant { status: string; mutatorName?: string; replacement?: string; location?: { start: { line: number } } }
interface StrykerFile { mutants: StrykerMutant[] }
export interface SurvivingMutant { file: string; line: number; mutator: string; replacement: string; status: string }

export function mutationTargets(ctx: CheckContext): string[] {
  const cfg = ctx.config;
  const pool = ctx.base ? Object.entries(diffAgainst(ctx.cwd, ctx.base)).filter(([, d]) => d.status !== 'deleted').map(([f]) => f) : listFiles(ctx.cwd);
  const only = ctx.options.mutate;
  return pool.filter((f) => isProductFile(cfg, f) && cfg.mutation.tiers.includes(tierOf(cfg, f)) && existsSync(join(ctx.cwd, f)) && (!only || matchesAny(f, only)));
}

/** The Stryker config guardrails generates. Mutators are pinned by the exact Stryker version plus excludedMutations. */
export function strykerConfig(ctx: CheckContext, mutate: string[], cacheDir: string): Record<string, unknown> {
  const m = ctx.config.mutation;
  const vitestConfig = ['vitest.config.ts', 'vitest.config.mts', 'vitest.config.js', 'vitest.config.mjs'].find((f) => existsSync(join(ctx.cwd, f)));
  return {
    ...(vitestConfig ? { vitest: { configFile: vitestConfig } } : {}),
    $schema: './node_modules/@stryker-mutator/core/schema/stryker-schema.json',
    testRunner: 'vitest',
    plugins: ['@stryker-mutator/vitest-runner'],
    mutate,
    mutator: { excludedMutations: m.excludedMutations },
    reporters: ['json', 'clear-text'],
    jsonReporter: { fileName: join(cacheDir, 'mutation.json') },
    incremental: true,
    incrementalFile: join(cacheDir, 'stryker-incremental.json'),
    thresholds: { high: 90, low: m.break, break: null },
    timeoutMS: 60_000,
    concurrency: 2,
    // Not under node_modules: Vite/esbuild ignore tsconfig for files there (breaks JSX).
    tempDirName: '.stryker-tmp',
    cleanTempDir: true,
  };
}

type StrykerRun = { json: { files: Record<string, StrykerFile> } } | { error: string; code: number } | { skipped: string; detail?: string };

/** Run Stryker on targets. GUARDRAILS_MUTATION=1 lets a project's vitest config drop slow suites. */
function runStryker(ctx: CheckContext, targets: string[]): StrykerRun {
  if (!targets.length) return { skipped: 'skipped', detail: 'no mutation-tier files changed' };
  if (!existsSync(join(ctx.cwd, 'node_modules/@stryker-mutator/core')))
    return { skipped: 'not installed', detail: 'npm i -D -E @stryker-mutator/core @stryker-mutator/vitest-runner' };
  const cacheDir = join('node_modules', '.cache', 'guardrails');
  mkdirSync(join(ctx.cwd, cacheDir), { recursive: true });
  const confPath = join(cacheDir, 'stryker.config.json');
  writeFileSync(join(ctx.cwd, confPath), JSON.stringify(strykerConfig(ctx, targets, cacheDir), null, 2));
  const r = runCommand(`npx stryker run ${JSON.stringify(confPath)}`, ctx.cwd, { GUARDRAILS_MUTATION: '1' }, ctx.config.mutation.timeoutMinutes * 60_000);
  const report = join(ctx.cwd, cacheDir, 'mutation.json');
  if (!existsSync(report)) return { error: (r.stderr || r.stdout).split('\n').slice(-3).join(' '), code: r.code };
  return { json: JSON.parse(readFileSync(report, 'utf8')) };
}

/** Surviving / uncovered mutants for changed (or given) files, for guardrails_mutants. Always recomputed. */
export function survivingMutants(ctx: CheckContext): { mutants: SurvivingMutant[]; note?: string } {
  const r = runStryker(ctx, mutationTargets(ctx));
  if ('skipped' in r) return { mutants: [], note: `${r.skipped}${r.detail ? `: ${r.detail}` : ''}` };
  if ('error' in r) return { mutants: [], note: `Stryker failed (exit ${r.code}): ${r.error}` };
  const out: SurvivingMutant[] = [];
  for (const [file, f] of Object.entries(r.json.files))
    for (const m of f.mutants)
      if (m.status === 'Survived' || m.status === 'NoCoverage')
        out.push({ file: file.replace(`${ctx.cwd}/`, ''), line: m.location?.start.line ?? 0, mutator: m.mutatorName ?? '?', replacement: m.replacement ?? '', status: m.status });
  return { mutants: out };
}

/** TEST-8: Stryker on mutation-tier files (changed ones in diff mode). */
export function checkMutation(ctx: CheckContext): CheckOutput {
  const r = runStryker(ctx, mutationTargets(ctx));
  if ('skipped' in r) return { violations: [], metrics: [{ name: 'mutation', value: r.skipped, detail: r.detail }] };
  if ('error' in r) return { violations: [violation('TEST-8', '(mutation)', `Stryker did not produce a report (exit ${r.code}): ${r.error}`, 'noreport')] };
  const json = r.json;
  const v: Violation[] = [];
  let detected = 0;
  let valid = 0;
  const perFile: string[] = [];
  for (const [file, f] of Object.entries(json.files)) {
    const k = f.mutants.filter((x) => x.status === 'Killed' || x.status === 'Timeout').length;
    const n = f.mutants.filter((x) => ['Killed', 'Timeout', 'Survived', 'NoCoverage'].includes(x.status)).length;
    detected += k;
    valid += n;
    if (!n) continue;
    const score = (100 * k) / n;
    const rel = file.replace(`${ctx.cwd}/`, '');
    perFile.push(`${rel}: ${score.toFixed(1)}% (${k}/${n})`);
    if (score < ctx.config.mutation.break)
      v.push(violation('TEST-8', rel, `mutation score ${score.toFixed(1)}% is below ${ctx.config.mutation.break}% (${n - k} surviving/uncovered of ${n}). Add assertions that kill them.`, 'score'));
  }
  const total = valid ? Number(((100 * detected) / valid).toFixed(1)) : 0;
  return { violations: v, metrics: [{ name: 'mutation', value: total, detail: perFile.join('\n') }] };
}
