import { readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { checkArch } from '../src/checks/arch.js';
import { run } from '../src/runner.js';
import type { Metric } from '../src/types.js';
import { BOUNDARY_CONFIG, BOUNDARY_FILES, sortFindings, sortViolations, type ViolationRow } from './helpers/mut-arch.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z'));
});

afterEach(() => {
  vi.useRealTimers();
});

describe('ARCH-04 checkArch boundary, fixture, singleton, restricted-string, dep and reachability enforcement', () => {
  it('ARCH-04 enforces boundaries, fixture imports, singletons, restricted strings and shipped assets while honouring exemptions', async () => {
    const r = repo(BOUNDARY_FILES, BOUNDARY_CONFIG);
    const rep = await run({ cwd: r.dir, only: ['arch'], knip: false });
    expect([rep.ok, rep.counts, rep.metrics]).toEqual([false, { block: 13, warn: 3, known: 0 }, []]);
    expect(sortFindings(rep.findings)).toEqual([
      { rule: 'ARCH-1', file: 'src/pages/a.ts', line: null, message: 'imports src/raw/db.ts, crossing boundary "1".', fingerprint: '473dd60f6321', mode: 'block' },
      { rule: 'ARCH-1', file: 'src/pages/a.ts', line: null, message: 'imports src/secret/key.ts, crossing boundary "core-only". Use facade.', fingerprint: 'cf85f5997567', mode: 'block' },
      { rule: 'ARCH-1', file: 'src/views/b.ts', line: null, message: 'imports src/legacy/old.ts, crossing boundary "no-legacy". Drop legacy.', fingerprint: '4d2ea80c05b1', mode: 'block' },
      { rule: 'ARCH-1', file: 'src/views/b.ts', line: null, message: 'imports src/secret/token.ts, crossing boundary "core-only". Use facade.', fingerprint: 'bfb1163079a1', mode: 'block' },
      { rule: 'ARCH-2', file: 'src/pages/a.ts', line: null, message: 'defines dupConst, which is already defined in packages/web/app.ts. Keep one implementation and import it.', fingerprint: '49cf61a9cd24', mode: 'warn' },
      { rule: 'ARCH-2', file: 'src/views/b.ts', line: null, message: 'defines dupFn, which is already defined in src/pages/a.ts. Keep one implementation and import it.', fingerprint: 'c4ca76d8d0a6', mode: 'warn' },
      { rule: 'ARCH-2', file: 'sub/entry.ts', line: null, message: 'defines dupAsync, which is already defined in mods/plug.ts. Keep one implementation and import it.', fingerprint: 'f5986e5e225c', mode: 'warn' },
      { rule: 'ARCH-5', file: 'src/pages/a.ts', line: 1, message: 'matches /raw://ban/. Use client.', fingerprint: '4629c4f94fd7', mode: 'block' },
      { rule: 'ARCH-5', file: 'src/views/b.ts', line: 4, message: 'matches /raw://ban/. Use client.', fingerprint: 'dfdcda5929f7', mode: 'block' },
      { rule: 'ARCH-6', file: 'mods/plug.ts', line: null, message: 'imports fixture src/fixtures/fx.ts. Only tests and dev-only code may load fixtures.', fingerprint: 'f5574a56294d', mode: 'block' },
      { rule: 'ARCH-6', file: 'packages/web/app.ts', line: null, message: 'imports fixture src/fixtures/fx.ts. Only tests and dev-only code may load fixtures.', fingerprint: 'cda85bf2f3c5', mode: 'block' },
      { rule: 'ARCH-6', file: 'src/pages/a.ts', line: null, message: 'imports fixture src/fixtures/fx.ts. Only tests and dev-only code may load fixtures.', fingerprint: '1ae74440f400', mode: 'block' },
      { rule: 'ARCH-6', file: 'src/pages/a.ts', line: null, message: 'references fixture asset /fixtures/hero.png. Fixtures must not ship to every user.', fingerprint: '9661a9268f3d', mode: 'block' },
      { rule: 'ARCH-6', file: 'sub/entry.ts', line: null, message: 'imports fixture src/fixtures/fx.ts. Only tests and dev-only code may load fixtures.', fingerprint: '584cc0f774e2', mode: 'block' },
      { rule: 'ARCH-6', file: 'web/landing.html', line: null, message: 'references fixture asset /fixtures/hero.png. Fixtures must not ship to every user.', fingerprint: '3846523ae411', mode: 'block' },
      { rule: 'ARCH-6', file: 'web/theme.scss', line: null, message: 'references fixture asset /fixtures/hero.png. Fixtures must not ship to every user.', fingerprint: 'c380941892ba', mode: 'block' },
    ]);
  });

  it.each<{ label: string; files: Record<string, string>; config: object; expected: ViolationRow[] }>([
    {
      label: 'custom architecture.tsConfig without tsconfig.json',
      files: {
        'tsconfig.app.json': JSON.stringify({ compilerOptions: { baseUrl: '.', paths: { '#sec/*': ['src/secret/*'] } } }),
        'src/secret/x.ts': 'export const x = 1;\n',
        'src/app.ts': 'import { x } from "#sec/x";\nexport const app = x;\n',
      },
      config: { ...BASE_CONFIG, architecture: { tsConfig: 'tsconfig.app.json', forbidden: [{ name: 'custom', from: ['src/**'], to: ['src/secret/**'] }] } },
      expected: [{ rule: 'ARCH-1', file: 'src/app.ts', line: null, message: 'imports src/secret/x.ts, crossing boundary "custom".', fingerprint: '1caba02d365d' }],
    },
    {
      label: 'no tsconfig file on disk',
      files: { 'src/secret/x.ts': 'export const x = 1;\n', 'src/app.ts': 'import { x } from "./secret/x";\nexport const app = x;\n' },
      config: { ...BASE_CONFIG, architecture: { forbidden: [{ from: ['src/**'], to: ['src/secret/**'] }] } },
      expected: [{ rule: 'ARCH-1', file: 'src/app.ts', line: null, message: 'imports src/secret/x.ts, crossing boundary "0".', fingerprint: 'e6039eec9b51' }],
    },
    {
      label: 'no product or helper source files',
      files: { 'src/only.test.ts': 'it("CORE-01 ok", () => { expect(1).toBe(1); });\n' },
      config: BASE_CONFIG,
      expected: [],
    },
  ])('ARCH-04 resolves tsconfig options and empty roots: $label', async ({ files, config, expected }) => {
    const r = repo(files, config);
    const out = await checkArch(r.ctx());
    expect([sortViolations(out.violations), out.metrics]).toEqual([expected, []]);
  });

  it.each<{
    label: string;
    baseFiles: Record<string, string>;
    headFiles: Record<string, string>;
    expectedViolations: ViolationRow[];
    expectedMetrics: Metric[];
  }>([
    {
      label: 'mixed registry/non-registry specs, allowAdded, optionalDependencies and node: builtins',
      baseFiles: {
        'package.json': JSON.stringify({ dependencies: { 'kept-allow': '1.0.0', 'newly-allow': '1.0.0' } }),
        'guardrails.config.json': JSON.stringify({ ...BASE_CONFIG, deps: { allow: ['kept-allow'] } }),
      },
      headFiles: {
        'package.json': JSON.stringify({
          dependencies: {
            'kept-allow': '1.0.0',
            'newly-allow': '1.0.0',
            'unapproved-pkg': '^2.0.0',
            'aliased-npm': 'npm:@scope/pkg@1.0.0',
            'tar-http': 'http://example.com/p.tgz',
            'at-ver': 'user/repo@1.0.0',
            'gh-frag': 'my-org/my-repo#v1.0',
            'gh-plain': 'org2/repo2',
            'node:events': '1.0.0',
          },
          optionalDependencies: { 'opt-local': 'portal:./p' },
        }),
        'guardrails.config.json': JSON.stringify({
          ...BASE_CONFIG,
          deps: { allow: ['kept-allow', 'newly-allow', 'ghost-allow', 'aliased-npm', 'tar-http', 'at-ver', 'gh-frag', 'gh-plain', 'node:events'] },
        }),
      },
      expectedViolations: [
        { rule: 'ARCH-4', file: 'package.json', line: null, message: 'adds dependency "unapproved-pkg" (not in deps.allow). A code owner must add it to deps.allow (CODEOWNER-protected config).', fingerprint: '86ccadca15f0' },
        { rule: 'ARCH-4', file: 'package.json', line: null, message: 'adds dependency "opt-local" (not in deps.allow). A code owner must add it to deps.allow (CODEOWNER-protected config).', fingerprint: '9718d5184234' },
      ],
      expectedMetrics: [
        { name: 'deps.added', value: 8, detail: 'unapproved-pkg, aliased-npm, tar-http, at-ver, gh-frag, gh-plain, node:events, opt-local' },
        { name: 'deps.registry', value: 'skipped', detail: 'offline: unapproved-pkg, aliased-npm, at-ver, newly-allow, ghost-allow not checked against npm' },
      ],
    },
    {
      label: 'new package.json vs missing base package.json and corrupt base config',
      baseFiles: { 'guardrails.config.json': '{corrupt-base-json' },
      headFiles: {
        'package.json': JSON.stringify({ devDependencies: { 'brand-new': '1.0.0' } }),
        'guardrails.config.json': JSON.stringify(BASE_CONFIG),
      },
      expectedViolations: [
        { rule: 'ARCH-4', file: 'package.json', line: null, message: 'adds dependency "brand-new" (not in deps.allow). A code owner must add it to deps.allow (CODEOWNER-protected config).', fingerprint: '0541475254e7' },
      ],
      expectedMetrics: [
        { name: 'deps.added', value: 1, detail: 'brand-new' },
        { name: 'deps.registry', value: 'skipped', detail: 'offline: brand-new not checked against npm' },
      ],
    },
    {
      label: 'diff mode without package.json on head',
      baseFiles: {},
      headFiles: {},
      expectedViolations: [],
      expectedMetrics: [],
    },
  ])('ARCH-04 checks newDeps and registry filtering: $label', async ({ baseFiles, headFiles, expectedViolations, expectedMetrics }) => {
    const r = repo(baseFiles).commit();
    r.write(headFiles);
    const out = await checkArch(r.ctx('main'));
    expect([sortViolations(out.violations), out.metrics]).toEqual([expectedViolations, expectedMetrics]);
  });

  it('ARCH-04 reports unreachable modules across sourceRoots in warn mode via knip and skips when knip is false', async () => {
    const unreachableMsg = 'is not reachable from any production entry (architecture.entries). Wire it in or delete it and its tests.';
    const r = repo(
      {
        'package.json': JSON.stringify({ name: 'p', type: 'module', devDependencies: { vitest: '1.6.1' } }),
        'vitest.setup.ts': "import { beforeEach, expect } from 'vitest';\nimport './src/setup-only.js';\nbeforeEach(() => { expect.hasAssertions(); });\n",
        'src/main.ts': 'import { used } from "./used.js";\nexport const app = used;\n',
        'src/used.ts': 'export const used = 1;\n',
        'src/orphan.ts': 'export const orphan = 2;\n',
        'src/setup-only.ts': 'export const setupOnly = 3;\n',
        'packages/web/orphan.ts': 'export const webOrphan = 4;\n',
        'sub/orphan.ts': 'export const subOrphan = 5;\n',
        'mods/orphan.ts': 'export const modOrphan = 6;\n',
        'mods/notes.txt': 'not a source file\n',
        'src/main.test.ts': 'it("CORE-01 ok", () => { expect(1).toBe(1); });\n',
        'src/helpers/h.ts': 'export const h = 1;\n',
        'src/fixtures/f.ts': 'export const f = 1;\n',
        '.github/workflows/odd.yml': 'name: odd on:\n  push: [:\n',
      },
      {
        ...BASE_CONFIG,
        sourceRoots: ['src', 'packages/web', 'sub/', 'mods/*'],
        architecture: { entries: ['src/main.ts'] },
        tests: { globs: ['**/*.test.ts'], helperGlobs: ['src/helpers/**'] },
      },
    ).commit();

    const skipped = await run({ cwd: r.dir, only: ['arch'], knip: false });
    expect([skipped.ok, skipped.findings, skipped.metrics]).toEqual([true, [], []]);

    const active = await run({ cwd: r.dir, only: ['arch'], knip: true });
    expect([active.ok, active.counts, active.metrics]).toEqual([true, { block: 0, warn: 5, known: 0 }, []]);
    expect(sortFindings(active.findings)).toEqual([
      { rule: 'ARCH-3', file: 'mods/orphan.ts', line: null, message: unreachableMsg, fingerprint: 'f23479fa589c', mode: 'warn' },
      { rule: 'ARCH-3', file: 'packages/web/orphan.ts', line: null, message: unreachableMsg, fingerprint: '558878ca6496', mode: 'warn' },
      { rule: 'ARCH-3', file: 'src/orphan.ts', line: null, message: unreachableMsg, fingerprint: '764a164e34aa', mode: 'warn' },
      { rule: 'ARCH-3', file: 'src/setup-only.ts', line: null, message: unreachableMsg, fingerprint: '274ed05bba2c', mode: 'warn' },
      { rule: 'ARCH-3', file: 'sub/orphan.ts', line: null, message: unreachableMsg, fingerprint: '5dc88bbdf68e', mode: 'warn' },
    ]);
  });

  it('ARCH-04 skips knip without entries, surfaces knip failures as a single-line metric, and removes temp dirs', async () => {
    const noEntries = repo({ 'src/orphan.ts': 'export const orphan = 1;\n' }, { ...BASE_CONFIG, architecture: { entries: [] } });
    const emptyOut = await checkArch({ ...noEntries.ctx(), options: { clockShift: false, mutation: false, knip: true } }, []);
    expect(emptyOut).toEqual({ violations: [], metrics: [] });

    const brokenCtx = { ...noEntries.ctx(), cwd: join(noEntries.dir, 'missing-cwd'), config: { ...noEntries.ctx().config, architecture: { ...noEntries.ctx().config.architecture, entries: ['src/main.ts'] } }, options: { clockShift: false, mutation: false, knip: true } };
    const countKnipTmp = () => readdirSync(tmpdir()).filter((n) => n.startsWith('guardrails-knip-')).length;
    const before = countKnipTmp();
    const errOut = await checkArch(brokenCtx, []);
    expect(errOut.violations).toEqual([]);
    expect(errOut.metrics).toHaveLength(1);
    const metric = errOut.metrics?.[0];
    expect([metric?.name, metric?.value, metric?.detail?.includes('\n'), (metric?.detail?.length ?? 0) > 10, metric?.detail?.includes('ENOENT')]).toEqual([
      'knip',
      'error',
      false,
      true,
      true,
    ]);
    expect(countKnipTmp()).toBe(before);
  });
});
