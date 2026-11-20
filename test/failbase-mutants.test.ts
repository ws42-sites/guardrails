import { existsSync, mkdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { changedTests, checkFailsOnBase, titleMatcher } from '../src/checks/failbase.js';
import { fingerprint, mergeBase } from '../src/util.js';
import { createFakeRunner, failbaseConfig, gitRun, initRepo, testFile, worktreeCount } from './helpers/mut-failbase.js';

describe('FAILBASE-02 checkFailsOnBase, changedTests and titleMatcher', () => {
  it.each<[string, string, boolean]>([
    ['CORE-01 %s', 'CORE-01 hello', true],
    ['CORE-01 %s', 'CORE-01 ', true],
    ['CORE-01 %d', 'CORE-01 42', true],
    ['CORE-01 %i', 'CORE-01 -7', true],
    ['CORE-01 %f', 'CORE-01 3.14', true],
    ['CORE-01 %j', 'CORE-01 {"a":1}', true],
    ['CORE-01 %o', 'CORE-01 [1]', true],
    ['CORE-01 %O', 'CORE-01 {a:1}', true],
    ['CORE-01 %#', 'CORE-01 0', true],
    ['CORE-01 %%', 'CORE-01 %', true],
    ['CORE-01 %z', 'CORE-01 %z', true],
    ['CORE-01 %z', 'CORE-01 anything', false],
    ['CORE-01 ${} end', 'CORE-01 any value end', true],
    ['CORE-01 ${} end', 'CORE-01  end', true],
    ['CORE-01 ${a} end', 'CORE-01 ${a} end', true],
    ['CORE-01 ${a} end', 'CORE-01 val end', false],
    ['CORE-01 $foo_bar', 'CORE-01 99', true],
    ['CORE-01 $A_Z', 'CORE-01 ok', true],
    ['CORE-01 $1', 'CORE-01 $1', true],
    ['CORE-01 $1', 'CORE-01 x', false],
    ['CORE-01 %s and %d and $x', 'CORE-01 a and 2 and z', true],
    ['CORE-01 a.b*c+d?e^f$g{1}(h)|[i]\\j', 'CORE-01 a.b*c+d?e^f$g{1}(h)|[i]\\j', true],
    ['CORE-01 a.b', 'CORE-01 axb', false],
    ['CORE-01 a+b', 'CORE-01 aab', false],
    ['CORE-01 a?b', 'CORE-01 b', false],
    ['CORE-01 a*b', 'CORE-01 b', false],
    ['CORE-01 a|b', 'CORE-01 a', false],
    ['CORE-01 [a]', 'CORE-01 a', false],
    ['CORE-01 (a)', 'CORE-01 a', false],
    ['CORE-01 a{2}', 'CORE-01 aa', false],
    ['CORE-01 a\\d', 'CORE-01 a5', false],
    ['CORE-01 exact', 'pre CORE-01 exact', false],
    ['CORE-01 exact', 'CORE-01 exact post', false],
  ])('FAILBASE-02 titleMatcher matches placeholders, escapes metacharacters and anchors: %s vs %s', (pattern, actual, expected) => {
    expect(titleMatcher(pattern).test(actual)).toBe(expected);
  });

  it('FAILBASE-02 changedTests finds new, body-changed and renamed tests across ts and tsx files', () => {
    const r = initRepo({
      'src/core/a.test.ts': testFile(['CORE-01 keep', 'expect(1).toBe(1);'], ['CORE-01 edit', 'expect(1).toBe(1);'], ['CORE-01 old', 'expect(2).toBe(2);'], ['CORE-01 drop', 'expect(3).toBe(3);']),
      'src/core/view.test.tsx': "import { expect, it } from 'vitest';\nit('CORE-01 jsx', () => { const x = <div />; expect(x).toBeDefined(); });\n",
    }, failbaseConfig('node -e ""'));
    r.write({
      'src/core/a.test.ts': testFile(['CORE-01 keep', 'expect(1).toBe(1);'], ['CORE-01 edit', 'expect(1).toBe(2);'], ['CORE-01 renamed', 'expect(2).toBe(2);'], ['CORE-01 added', 'expect(4).toBe(4);']),
      'src/core/new.test.tsx': "import { expect, it } from 'vitest';\nit('CORE-01 fresh', () => { const el = <span />; expect(el).toBeDefined(); });\n",
    });
    const found = changedTests(r.ctx('main'), mergeBase(r.dir, 'main'), ['src/core/a.test.ts', 'src/core/view.test.tsx', 'src/core/new.test.tsx']);
    expect(found.map(({ file, test }) => ({ file, title: test.title, specId: test.specId, line: test.line }))).toEqual([
      { file: 'src/core/a.test.ts', title: 'CORE-01 edit', specId: 'CORE-01', line: 3 },
      { file: 'src/core/a.test.ts', title: 'CORE-01 renamed', specId: 'CORE-01', line: 4 },
      { file: 'src/core/a.test.ts', title: 'CORE-01 added', specId: 'CORE-01', line: 5 },
      { file: 'src/core/new.test.tsx', title: 'CORE-01 fresh', specId: 'CORE-01', line: 2 },
    ]);
  });

  it('FAILBASE-02 checkFailsOnBase skips with no base, product-only edits, deleted tests or comment-only test edits', () => {
    const runner = createFakeRunner();
    const r = initRepo({
      'src/core/add.ts': '// BASE\n',
      'src/core/keep.test.ts': testFile(['CORE-01 kept']),
      'src/core/gone.test.ts': testFile(['CORE-01 gone']),
      'test/helpers/gone.ts': 'export const h = 1;\n',
    }, failbaseConfig(runner.command));
    expect(checkFailsOnBase(r.ctx(null))).toEqual({ violations: [] });
    const skipped = { violations: [], metrics: [{ name: 'failsOnBase', value: 'skipped', detail: 'no new or changed tests' }] };
    r.write({ 'src/core/add.ts': '// HEAD\n' });
    expect(checkFailsOnBase(r.ctx('main'))).toEqual(skipped);
    r.remove('src/core/gone.test.ts', 'test/helpers/gone.ts');
    r.write({ 'src/core/keep.test.ts': `// comment only\n${testFile(['CORE-01 kept'])}` });
    expect(checkFailsOnBase(r.ctx('main'))).toEqual(skipped);
    expect(runner.invocations()).toEqual([]);
  });

  it('FAILBASE-02 checkFailsOnBase runs base worktree and head, copies helpers/config, handles node_modules, clears clock env and cleans up', () => {
    const runner = createFakeRunner();
    const r = initRepo({
      'src/core/add.ts': '// BASE\n',
      'src/core/unchanged.test.ts': testFile(['CORE-01 untouched']),
      'src/core/deleted.test.ts': testFile(['CORE-01 deleted']),
      'test/helpers/deleted.ts': 'export const old = 1;\n',
    }, failbaseConfig(runner.command));
    gitRun(r.dir, 'checkout', '-b', 'feat');
    gitRun(r.dir, 'checkout', 'main');
    r.write({ 'src/core/add.ts': '// MAIN_AFTER_BRANCH\n' }).commit('main-ahead');
    gitRun(r.dir, 'checkout', 'feat');

    const headConfig = failbaseConfig(runner.command, { 'CORE-01': { tier: 'T1', behaviour: 'head updated behaviour' } });
    r.remove('src/core/deleted.test.ts', 'test/helpers/deleted.ts');
    r.write({
      'guardrails.config.json': JSON.stringify(headConfig),
      'src/core/add.ts': '// HEAD\n',
      'src/core/unchanged.test.ts': `// comment only\n${testFile(['CORE-01 untouched'])}`,
      'src/core/add.test.ts': testFile(['CORE-01 adds %d'], ['CORE-01 helper check']),
      'src/newdir/sub/extra.test.ts': testFile(['CORE-01 nested']),
      'test/helpers/nested/h.ts': '// HEAD_HELPER\n',
    });
    runner.setPlan({
      mode: 'normal',
      files: {
        'src/core/add.test.ts': {
          nameFormat: 'abs',
          assertions: [
            { title: 'CORE-01 adds 1', pass: true },
            { title: 'CORE-01 adds 2', anyPresent: [{ file: 'src/core/add.ts', contains: 'HEAD' }, { file: 'src/core/add.ts', contains: 'MAIN_AFTER_BRANCH' }] },
            { title: 'CORE-01 helper check', allPresent: [{ file: 'test/helpers/nested/h.ts', contains: 'HEAD_HELPER' }, { file: 'src/core/add.ts', contains: 'HEAD' }] },
          ],
        },
        'src/newdir/sub/extra.test.ts': {
          nameFormat: 'winRel',
          assertions: [{ title: 'CORE-01 nested', allPresent: [{ file: 'src/core/add.ts', contains: 'HEAD' }] }],
        },
      },
    });

    const prevClock = process.env.GUARDRAILS_CLOCK_SHIFT_YEARS;
    process.env.GUARDRAILS_CLOCK_SHIFT_YEARS = '5';
    let out: ReturnType<typeof checkFailsOnBase>;
    try { out = checkFailsOnBase(r.ctx('main')); } finally {
      if (prevClock === undefined) delete process.env.GUARDRAILS_CLOCK_SHIFT_YEARS;
      else process.env.GUARDRAILS_CLOCK_SHIFT_YEARS = prevClock;
    }

    expect(out).toEqual({ violations: [], metrics: [{ name: 'failsOnBase', value: '3/3', detail: '3 new/changed tests fail on base as required' }] });
    const [baseRun, headRun] = runner.invocations();
    const tmpDir = dirname(baseRun.cwd);
    expect([basename(baseRun.cwd), basename(tmpDir).startsWith('guardrails-base-'), realpathSync(dirname(tmpDir))]).toEqual(['base', true, realpathSync(tmpdir())]);
    expect([existsSync(tmpDir), worktreeCount(r.dir)]).toEqual([false, 1]);
    expect(baseRun.args[0]).toBe('--reporter=json');
    expect(baseRun.args[1].startsWith('--outputFile=')).toBe(true);
    expect(resolve(baseRun.args[1].slice('--outputFile='.length))).toBe(resolve(join(tmpDir, 'base.json')));
    expect(resolve(headRun.args[1].slice('--outputFile='.length))).toBe(resolve(join(tmpDir, 'head.json')));
    expect(baseRun.args.slice(2)).toEqual(['src/core/add.test.ts', 'src/newdir/sub/extra.test.ts']);
    expect([baseRun.clockEnv, headRun.clockEnv, baseRun.hasNodeModules, baseRun.configText]).toEqual(['', '', false, JSON.stringify(headConfig)]);

    runner.clearLog();
    mkdirSync(join(r.dir, 'node_modules', 'pkg'), { recursive: true });
    writeFileSync(join(r.dir, 'node_modules', 'pkg', 'ok.txt'), 'nm-ok');
    expect(checkFailsOnBase(r.ctx('main')).violations).toEqual([]);
    const nmBase = runner.invocations()[0];
    expect([nmBase.hasNodeModules, nmBase.nodeModulesMarker, existsSync(dirname(nmBase.cwd)), worktreeCount(r.dir)]).toEqual([true, 'nm-ok', false, 1]);
  });

  it('FAILBASE-02 checkFailsOnBase blocks base-passing tests with exact diagnostics and honours regressionGuard and labels', () => {
    const runner = createFakeRunner();
    const cfg = failbaseConfig(runner.command, { 'LIB-01': { tier: 'T2', behaviour: 'lib formats text', regressionGuard: true } });
    const r = initRepo({ 'src/core/add.ts': '// BASE\n' }, cfg);
    r.write({
      'src/core/add.ts': '// HEAD\n',
      'test/helpers/shared.ts': '// HELPER_COPIED\n',
      'src/core/pass.test.ts': testFile(['CORE-01 already green'], ['plain test without id'], ['LIB-01 guarded'], ['CORE-01 real fix']),
    });
    runner.setPlan({
      mode: 'normal',
      files: {
        'src/core/pass.test.ts': {
          nameFormat: 'rel',
          assertions: [
            { title: 'CORE-01 already green', pass: true },
            { title: 'plain test without id', pass: true },
            { title: 'LIB-01 guarded', allPresent: [{ file: 'test/helpers/shared.ts', contains: 'HELPER_COPIED' }] },
            { title: 'CORE-01 real fix', allPresent: [{ file: 'src/core/add.ts', contains: 'HEAD' }] },
          ],
        },
      },
    });

    expect(checkFailsOnBase(r.ctx('main'))).toEqual({
      violations: [
        {
          rule: 'TEST-11',
          file: 'src/core/pass.test.ts',
          line: 2,
          message: '"CORE-01 already green" already passes on main without this change, so it does not test the change. Assert the new behaviour, or mark CORE-01 regressionGuard / have a code owner add the "guardrails:existing-behaviour-tests-approved" label, which approves new tests that lock in existing behaviour.',
          fingerprint: fingerprint('TEST-11', 'src/core/pass.test.ts', 'base:CORE-01 already green'),
        },
        {
          rule: 'TEST-11',
          file: 'src/core/pass.test.ts',
          line: 3,
          message: '"plain test without id" already passes on main without this change, so it does not test the change. Assert the new behaviour, or mark the spec regressionGuard / have a code owner add the "guardrails:existing-behaviour-tests-approved" label, which approves new tests that lock in existing behaviour.',
          fingerprint: fingerprint('TEST-11', 'src/core/pass.test.ts', 'base:plain test without id'),
        },
      ],
      metrics: [{ name: 'failsOnBase', value: '1/4', detail: '1 new/changed tests fail on base as required, 1 allowed as regression guards/characterization' }],
    });
    const allowed = { violations: [], metrics: [{ name: 'failsOnBase', value: '1/4', detail: '1 new/changed tests fail on base as required, 3 allowed as regression guards/characterization' }] };
    expect(checkFailsOnBase(r.ctx('main', ['guardrails:existing-behaviour-tests-approved']))).toEqual(allowed);
    expect(checkFailsOnBase(r.ctx('main', ['allow-characterization']))).toEqual(allowed);
  });

  it('FAILBASE-02 checkFailsOnBase blocks failing or missing head results and handles empty or absent JSON reports', () => {
    const runner = createFakeRunner();
    const r = initRepo({ 'src/core/add.ts': '// BASE\n' }, failbaseConfig(runner.command));
    r.write({
      'src/core/add.ts': '// HEAD\n',
      'src/core/fail.test.ts': testFile(['CORE-01 row %d'], ['CORE-01 missing head'], ['CORE-01 missing base']),
    });
    runner.setPlan({
      mode: 'normal',
      files: {
        'src/core/fail.test.ts': {
          nameFormat: 'abs',
          assertions: [
            { title: 'CORE-01 row 1', pass: true },
            { title: 'CORE-01 row 2', pass: false },
            { title: 'CORE-01 missing base', pass: true, onlyWhen: { file: 'src/core/add.ts', contains: 'HEAD' } },
          ],
        },
      },
    });
    expect(checkFailsOnBase(r.ctx('main'))).toEqual({
      violations: [
        { rule: 'TEST-11', file: 'src/core/fail.test.ts', line: 2, message: '"CORE-01 row %d" does not pass on this branch.', fingerprint: fingerprint('TEST-11', 'src/core/fail.test.ts', 'head:CORE-01 row %d') },
        { rule: 'TEST-11', file: 'src/core/fail.test.ts', line: 3, message: '"CORE-01 missing head" does not pass on this branch.', fingerprint: fingerprint('TEST-11', 'src/core/fail.test.ts', 'head:CORE-01 missing head') },
      ],
      metrics: [{ name: 'failsOnBase', value: '1/3', detail: '1 new/changed tests fail on base as required' }],
    });
    for (const mode of ['missingOutput', 'emptyObject', 'omitAssertions'] as const) {
      runner.setPlan({ mode });
      const res = checkFailsOnBase(r.ctx('main'));
      expect(res.violations.map((v) => v.message)).toEqual([
        '"CORE-01 row %d" does not pass on this branch.',
        '"CORE-01 missing head" does not pass on this branch.',
        '"CORE-01 missing base" does not pass on this branch.',
      ]);
      expect(res.metrics).toEqual([{ name: 'failsOnBase', value: '0/3', detail: '0 new/changed tests fail on base as required' }]);
    }
  });

  it('FAILBASE-02 checkFailsOnBase tolerates absent config file and reports error metric while still cleaning up on failure', () => {
    const runner = createFakeRunner();
    const r = initRepo({ 'src/core/add.ts': '// BASE\n' }, failbaseConfig(runner.command));
    r.write({ 'src/core/add.ts': '// HEAD\n', 'src/core/err.test.ts': testFile(['CORE-01 fix']) });
    const ctx = r.ctx('main');
    rmSync(join(r.dir, 'guardrails.config.json'));
    runner.setPlan({
      mode: 'normal',
      files: { 'src/core/err.test.ts': { assertions: [{ title: 'CORE-01 fix', allPresent: [{ file: 'src/core/add.ts', contains: 'HEAD' }] }] } },
    });
    expect(checkFailsOnBase(ctx)).toEqual({
      violations: [],
      metrics: [{ name: 'failsOnBase', value: '1/1', detail: '1 new/changed tests fail on base as required' }],
    });

    runner.clearLog();
    runner.setPlan({ mode: 'invalidJson' });
    const errOut = checkFailsOnBase(ctx);
    const errRuns = runner.invocations();
    expect([errOut.violations, errOut.metrics?.length, errOut.metrics?.[0].name, errOut.metrics?.[0].value]).toEqual([[], 1, 'failsOnBase', 'error']);
    expect([errOut.metrics?.[0].detail?.includes('\n'), (errOut.metrics?.[0].detail?.length ?? 0) > 0]).toEqual([false, true]);
    expect([existsSync(dirname(errRuns[0].cwd)), worktreeCount(r.dir)]).toEqual([false, 1]);
  });
});
