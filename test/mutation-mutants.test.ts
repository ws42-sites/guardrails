import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const strykerState = vi.hoisted(() => ({
  calls: [] as { cmd: string; cwd: string; env: Record<string, string>; timeoutMs: number; confText: string | null }[],
  next: { code: 0, stdout: '', stderr: '', rawReport: null as string | null },
}));

vi.mock('../src/checks/run.js', () => ({
  runCommand: (cmd: string, cwd: string, env: Record<string, string> = {}, timeoutMs = 900_000) => {
    const confFile = join(cwd, 'node_modules', '.cache', 'guardrails', 'stryker.config.json');
    const confText = existsSync(confFile) ? readFileSync(confFile, 'utf8') : null;
    strykerState.calls.push({ cmd, cwd, env, timeoutMs, confText });
    if (strykerState.next.rawReport !== null) {
      writeFileSync(join(cwd, 'node_modules', '.cache', 'guardrails', 'mutation.json'), strykerState.next.rawReport);
    }
    return { code: strykerState.next.code, stdout: strykerState.next.stdout, stderr: strykerState.next.stderr };
  },
}));

import { checkMutation, mutationTargets, strykerConfig, survivingMutants, type SurvivingMutant } from '../src/checks/mutation.js';
import { parseConfig } from '../src/config.js';
import type { CheckOutput } from '../src/types.js';
import { fingerprint } from '../src/util.js';
import {
  buildSeededRows, CANARY, checkoutBranch, HOSTILE_TEXT, installStryker, LONG_TEXT,
  mixedSurvivorsReport, multiFileScoreReport, mutConfig, plainCtx, scoreViolation,
} from './helpers/mut-mutation.js';
import { repo } from './helpers/repo.js';

const SEEDED_ROWS = buildSeededRows();

describe('MUT-01 checkMutation, survivingMutants, strykerConfig and mutationTargets', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z'));
    strykerState.calls = [];
    strykerState.next = { code: 0, stdout: '', stderr: '', rawReport: null };
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it.each<{ label: string; base: string | null; tiers: ('T1' | 'T2' | 'T3')[]; mutateOpt?: string[]; expected: string[] }>([
    { label: 'full repo selects existing T1 product files and skips missing, excluded, T2, T3 and non-product', base: null, tiers: ['T1'], expected: ['src/core/dirswap.ts', 'src/core/kept.ts', 'src/core/mod.ts'] },
    { label: 'full repo includes T2 when configured in mutation.tiers', base: null, tiers: ['T1', 'T2'], expected: ['src/core/dirswap.ts', 'src/core/kept.ts', 'src/core/mod.ts', 'src/lib/util.ts'] },
    { label: 'diff mode includes branch commits vs base and drops unchanged, deleted, directory-swapped, excluded, T2 and non-product', base: 'main', tiers: ['T1'], expected: ['src/core/branch.ts', 'src/core/mod.ts', 'src/core/added.ts'] },
    { label: 'ctx.options.mutate filters candidate targets by glob', base: 'main', tiers: ['T1'], mutateOpt: ['**/added.ts'], expected: ['src/core/added.ts'] },
    { label: 'empty ctx.options.mutate list selects nothing', base: null, tiers: ['T1'], mutateOpt: [], expected: [] },
    { label: 'empty mutation.tiers selects nothing', base: null, tiers: [], expected: [] },
  ])('MUT-01 mutationTargets filters by base diff, tier, exclude, mutate option and existence: $label', ({ base, tiers, mutateOpt, expected }) => {
    const r = repo(
      {
        'src/core/kept.ts': 'export const kept = 1;\n',
        'src/core/mod.ts': 'export const mod = 1;\n',
        'src/core/del.ts': 'export const del = 1;\n',
        'src/core/dirswap.ts': 'export const swap = 1;\n',
        'src/core/excluded.ts': 'export const ex = 1;\n',
        'src/core/gen/sub.ts': 'export const gen = 1;\n',
        'src/lib/util.ts': 'export const util = 1;\n',
        'src/ui/view.tsx': 'export const view = 1;\n',
        'src/core/readme.md': '# doc\n',
        'src/core/unit.test.ts': 'export {};\n',
        'test/helpers/h.ts': 'export const h = 1;\n',
        'outside/other.ts': 'export const o = 1;\n',
      },
      mutConfig({ tiers, exclude: ['src/core/excluded.ts', 'src/core/gen/**'] }),
    ).commit('init');
    if (base) {
      checkoutBranch(r.dir, 'feat');
      r.write({ 'src/core/branch.ts': 'export const br = 1;\n' }).commit('branch-commit');
      r.remove('src/core/del.ts', 'src/core/dirswap.ts');
      mkdirSync(join(r.dir, 'src', 'core', 'dirswap.ts'));
      r.write({
        'src/core/mod.ts': 'export const mod = 2;\n',
        'src/core/added.ts': 'export const added = 1;\n',
        'src/core/excluded.ts': 'export const ex = 2;\n',
        'src/lib/util.ts': 'export const util = 2;\n',
        'src/core/data.json': '{"a":1}\n',
      });
    } else {
      r.remove('src/core/del.ts');
    }
    const ctx = r.ctx(base);
    if (mutateOpt !== undefined) ctx.options.mutate = mutateOpt;
    expect(mutationTargets(ctx)).toEqual(expected);
  });

  it.each<{ label: string; presentConfigs: string[]; expectedVitest: string | null; excludedMutations: string[]; breakPct: number; cacheDir: string }>([
    { label: 'vitest.config.ts takes precedence', presentConfigs: ['vitest.config.ts', 'vitest.config.mts', 'vitest.config.js', 'vitest.config.mjs'], expectedVitest: 'vitest.config.ts', excludedMutations: ['StringLiteral', 'Regex'], breakPct: 85, cacheDir: join('node_modules', '.cache', 'guardrails') },
    { label: 'vitest.config.mts wins when .ts absent', presentConfigs: ['vitest.config.mts', 'vitest.config.js', 'vitest.config.mjs'], expectedVitest: 'vitest.config.mts', excludedMutations: ['BlockStatement'], breakPct: 70, cacheDir: join('cache', 'one') },
    { label: 'vitest.config.js wins when .ts and .mts absent', presentConfigs: ['vitest.config.js', 'vitest.config.mjs'], expectedVitest: 'vitest.config.js', excludedMutations: [], breakPct: 65, cacheDir: join('cache', 'two') },
    { label: 'vitest.config.mjs detected when alone', presentConfigs: ['vitest.config.mjs'], expectedVitest: 'vitest.config.mjs', excludedMutations: ['ConditionalExpression'], breakPct: 90, cacheDir: join('cache', 'three') },
    { label: 'omits vitest block when no vitest config exists', presentConfigs: [], expectedVitest: null, excludedMutations: [], breakPct: 0, cacheDir: join('cache', 'none') },
  ])('MUT-01 strykerConfig builds exact config across vitest config variants and thresholds: $label', ({ presentConfigs, expectedVitest, excludedMutations, breakPct, cacheDir }) => {
    const ctx = plainCtx(Object.fromEntries(presentConfigs.map((f) => [f, 'export default {};\n'])), { excludedMutations, break: breakPct });
    const targets = ['src/core/a.ts', 'src/core/b.ts'];
    const actual = strykerConfig(ctx, targets, cacheDir);
    expect('vitest' in actual).toBe(expectedVitest !== null);
    expect(actual).toEqual({
      ...(expectedVitest ? { vitest: { configFile: expectedVitest } } : {}),
      $schema: './node_modules/@stryker-mutator/core/schema/stryker-schema.json',
      testRunner: 'vitest',
      plugins: ['@stryker-mutator/vitest-runner'],
      mutate: targets,
      mutator: { excludedMutations },
      reporters: ['json', 'clear-text'],
      jsonReporter: { fileName: join(cacheDir, 'mutation.json') },
      incremental: true,
      incrementalFile: join(cacheDir, 'stryker-incremental.json'),
      thresholds: { high: 90, low: breakPct, break: null },
      timeoutMS: 60_000,
      concurrency: 2,
      tempDirName: '.stryker-tmp',
      cleanTempDir: true,
    });
  });

  it.each<{ label: string; mode: 'skipped' | 'not-installed' | 'stderr-tail' | 'short-stderr' | 'stdout-fallback' | 'empty-output' | 'corrupt-report' | 'null-files' }>([
    { label: 'skips when no mutation-tier files changed', mode: 'skipped' },
    { label: 'reports not installed when @stryker-mutator/core is absent', mode: 'not-installed' },
    { label: 'writes formatted config, passes env/timeout, and reports last 3 stderr lines when report missing', mode: 'stderr-tail' },
    { label: 'preserves both lines when stderr has fewer than 3 lines', mode: 'short-stderr' },
    { label: 'falls back to last 3 stdout lines when stderr is empty', mode: 'stdout-fallback' },
    { label: 'handles missing report when both stderr and stdout are empty', mode: 'empty-output' },
    { label: 'fails closed with SyntaxError when mutation.json is truncated mid-write', mode: 'corrupt-report' },
    { label: 'fails closed with TypeError when mutation.json files field is null', mode: 'null-files' },
  ])('MUT-01 runStryker handles skipped, missing package, command wiring, missing report and corrupt report: $label', ({ mode }) => {
    const r = repo({ 'src/core/a.ts': 'export const a = 1;\n' }, mutConfig({ timeoutMinutes: 23, excludedMutations: ['Regex'], break: 72 })).commit('base');
    if (mode === 'skipped') {
      installStryker(r.dir);
      const ctx = r.ctx('main');
      expect(checkMutation(ctx)).toEqual({ violations: [], metrics: [{ name: 'mutation', value: 'skipped', detail: 'no mutation-tier files changed' }] });
      expect(survivingMutants(ctx)).toEqual({ mutants: [], note: 'skipped: no mutation-tier files changed' });
      expect([strykerState.calls.length, existsSync(join(r.dir, 'node_modules', '.cache', 'guardrails'))]).toEqual([0, false]);
      return;
    }
    if (mode === 'not-installed') {
      mkdirSync(join(r.dir, 'node_modules', 'other-pkg'), { recursive: true });
      const ctx = r.ctx(null);
      expect(checkMutation(ctx)).toEqual({ violations: [], metrics: [{ name: 'mutation', value: 'not installed', detail: 'npm i -D -E @stryker-mutator/core @stryker-mutator/vitest-runner' }] });
      expect(survivingMutants(ctx)).toEqual({ mutants: [], note: 'not installed: npm i -D -E @stryker-mutator/core @stryker-mutator/vitest-runner' });
      expect(strykerState.calls).toEqual([]);
      return;
    }
    installStryker(r.dir);
    const ctx = r.ctx(null);
    if (mode === 'corrupt-report' || mode === 'null-files') {
      strykerState.next = { code: 0, stderr: '', stdout: '', rawReport: mode === 'corrupt-report' ? '{"files":' : '{"files":null}' };
      const errType = mode === 'corrupt-report' ? SyntaxError : TypeError;
      expect(() => checkMutation(ctx)).toThrow(errType);
      expect(() => survivingMutants(ctx)).toThrow(errType);
      return;
    }
    const spec = mode === 'stderr-tail'
      ? { code: 3, stderr: `${CANARY}\ne2\ne3\ne4\ne5`, stdout: `${CANARY}-out\ns2`, tail: 'e3 e4 e5' }
      : mode === 'short-stderr'
        ? { code: 5, stderr: 'e1\ne2', stdout: `${CANARY}-out`, tail: 'e1 e2' }
        : mode === 'stdout-fallback'
          ? { code: 9, stderr: '', stdout: `${CANARY}\no2\no3\no4\no5`, tail: 'o3 o4 o5' }
          : { code: 137, stderr: '', stdout: '', tail: '' };
    strykerState.next = { code: spec.code, stderr: spec.stderr, stdout: spec.stdout, rawReport: null };
    const outCheck = checkMutation(ctx);
    const outSurv = survivingMutants(ctx);
    expect(outCheck).toEqual({
      violations: [{
        rule: 'TEST-8',
        file: '(mutation)',
        line: undefined,
        message: `Stryker did not produce a report (exit ${spec.code}): ${spec.tail}`,
        fingerprint: fingerprint('TEST-8', '(mutation)', 'noreport'),
      }],
    });
    expect(outSurv).toEqual({ mutants: [], note: `Stryker failed (exit ${spec.code}): ${spec.tail}` });
    expect(JSON.stringify([outCheck, outSurv]).includes(CANARY)).toBe(false);
    const cacheDir = join('node_modules', '.cache', 'guardrails');
    const confPath = join(cacheDir, 'stryker.config.json');
    const expectedCall = {
      cmd: `npx stryker run ${JSON.stringify(confPath)}`,
      cwd: r.dir,
      env: { GUARDRAILS_MUTATION: '1' },
      timeoutMs: 23 * 60_000,
      confText: JSON.stringify(strykerConfig(ctx, ['src/core/a.ts'], cacheDir), null, 2),
    };
    expect(strykerState.calls).toEqual([expectedCall, expectedCall]);
  });

  it.each<{ label: string; breakPct: number; targetFiles: string[]; buildReport: (cwd: string) => string; expected: CheckOutput }>([
    {
      label: 'scores multiple files, strips cwd prefix, ignores non-targets and non-valid statuses, skips zero-valid file',
      breakPct: 70,
      targetFiles: ['src/core/a.ts', 'src/core/b.ts', 'src/core/zero.ts'],
      buildReport: multiFileScoreReport,
      expected: {
        violations: [scoreViolation('src/core/a.ts', '60.0', 70, 2, 5)],
        metrics: [{ name: 'mutation', value: 72.7, detail: 'src/core/a.ts: 60.0% (3/5)\nsrc/core/b.ts: 83.3% (5/6)' }],
      },
    },
    {
      label: 'at exact break threshold (75 === 75) passes without violation',
      breakPct: 75,
      targetFiles: ['src/core/a.ts'],
      buildReport: () => JSON.stringify({ files: { 'src/core/a.ts': { mutants: [{ status: 'Killed' }, { status: 'Killed' }, { status: 'Timeout' }, { status: 'Survived' }] } } }),
      expected: { violations: [], metrics: [{ name: 'mutation', value: 75, detail: 'src/core/a.ts: 75.0% (3/4)' }] },
    },
    {
      label: 'just below break threshold (75 < 75.1) blocks with TEST-8',
      breakPct: 75.1,
      targetFiles: ['src/core/a.ts'],
      buildReport: () => JSON.stringify({ files: { 'src/core/a.ts': { mutants: [{ status: 'Killed' }, { status: 'Killed' }, { status: 'Timeout' }, { status: 'NoCoverage' }] } } }),
      expected: { violations: [scoreViolation('src/core/a.ts', '75.0', 75.1, 1, 4)], metrics: [{ name: 'mutation', value: 75, detail: 'src/core/a.ts: 75.0% (3/4)' }] },
    },
    {
      label: 'just above break threshold (75 > 74.9) passes',
      breakPct: 74.9,
      targetFiles: ['src/core/a.ts'],
      buildReport: () => JSON.stringify({ files: { 'src/core/a.ts': { mutants: [{ status: 'Killed' }, { status: 'Killed' }, { status: 'Timeout' }, { status: 'Survived' }] } } }),
      expected: { violations: [], metrics: [{ name: 'mutation', value: 75, detail: 'src/core/a.ts: 75.0% (3/4)' }] },
    },
    {
      label: 'zero valid mutants across all targets reports total 0 and empty detail',
      breakPct: 70,
      targetFiles: ['src/core/a.ts'],
      buildReport: () => '{"files":{"src/core/a.ts":{"mutants":[{"status":"Ignored"},{"status":"CompileError"}]},"__proto__":{"mutants":[{"status":"Survived"}]}}}',
      expected: { violations: [], metrics: [{ name: 'mutation', value: 0, detail: '' }] },
    },
  ])('MUT-01 checkMutation scores valid mutants, strips cwd prefix and enforces break threshold: $label', ({ breakPct, targetFiles, buildReport, expected }) => {
    const r = repo(Object.fromEntries(targetFiles.map((f) => [f, 'export const v = 1;\n'])), mutConfig({ break: breakPct }));
    installStryker(r.dir);
    strykerState.next.rawReport = buildReport(r.dir);
    const out = checkMutation(r.ctx(null));
    expect(out).toEqual(expected);
    expect(JSON.stringify(out).includes(CANARY)).toBe(false);
  });

  it.each<{ label: string; buildReport: (cwd: string) => string; expected: { mutants: SurvivingMutant[]; note?: string } }>([
    {
      label: 'filters Survived and NoCoverage, strips cwd prefix, applies defaults and ignores non-targets and proto keys',
      buildReport: mixedSurvivorsReport,
      expected: {
        mutants: [
          { file: 'src/core/a.ts', line: 17, mutator: 'ConditionalExpression', replacement: HOSTILE_TEXT, status: 'Survived' },
          { file: 'src/core/a.ts', line: 0, mutator: '?', replacement: '', status: 'NoCoverage' },
          { file: 'src/core/a.ts', line: 51, mutator: 'StringLiteral', replacement: LONG_TEXT, status: 'NoCoverage' },
          { file: 'src/core/b.ts', line: 29, mutator: '', replacement: 'x || y', status: 'Survived' },
        ],
      },
    },
    {
      label: 'returns empty mutants without note when all mutants are killed or timed out',
      buildReport: () => JSON.stringify({ files: { 'src/core/a.ts': { mutants: [{ status: 'Killed', location: { start: { line: 1 } } }, { status: 'Timeout' }] } } }),
      expected: { mutants: [] },
    },
  ])('MUT-01 survivingMutants extracts Survived and NoCoverage mutants with fallback defaults: $label', ({ buildReport, expected }) => {
    const r = repo({ 'src/core/a.ts': 'export const a = 1;\n', 'src/core/b.ts': 'export const b = 1;\n' }, mutConfig());
    installStryker(r.dir);
    strykerState.next.rawReport = buildReport(r.dir);
    const out = survivingMutants(r.ctx(null));
    expect(out).toEqual(expected);
    expect('note' in out).toBe(false);
    expect(JSON.stringify(out).includes(CANARY)).toBe(false);
  });

  it('MUT-01 checkMutation and survivingMutants preserve invariants across seeded cases (seed=133742)', () => {
    const r = repo({ 'src/core/seed.ts': 'export const s = 1;\n' }, mutConfig());
    installStryker(r.dir);
    for (const row of SEEDED_ROWS) {
      const ctx = { ...r.ctx(null), config: parseConfig(mutConfig({ break: row.breakPct })) };
      strykerState.next.rawReport = JSON.stringify({ files: { 'src/core/seed.ts': { mutants: row.mutants } } });
      const checkOut = checkMutation(ctx);
      const survOut = survivingMutants(ctx);
      expect(checkOut).toEqual(row.expectedCheck);
      expect(survOut).toEqual({ mutants: row.expectedSurvivors });
      expect(JSON.stringify([checkOut, survOut]).includes(CANARY)).toBe(false);
    }
  });
});
