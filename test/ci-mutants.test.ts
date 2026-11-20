import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { checkCi } from '../src/checks/ci.js';
import type { Violation } from '../src/types.js';
import { fingerprint } from '../src/util.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const ON = { ...BASE_CONFIG, ci: { enabled: true } };
const NIGHTLY_YML = 'on:\n  schedule:\n    - cron: "0 7 * * *"\njobs:\n  nightly:\n    steps:\n      - run: npm run test:full\n';

const prViol = (file: string, line: number, cmd: string): Violation => ({
  rule: 'CI-1',
  file,
  line,
  message: `runs the full suite on every PR ("${cmd.slice(0, 60)}"). Run affected tests (npm run test:changed) on PRs and the full suite nightly.`,
  fingerprint: fingerprint('CI-1', file, `full:${cmd}`),
});

const nightlyViol = (): Violation => ({
  rule: 'CI-1',
  file: '.github/workflows',
  line: undefined,
  message: 'no scheduled (nightly) workflow runs the full suite. Add one that runs npm run test:full on a cron, or set ci.na.nightly with the reason.',
  fingerprint: fingerprint('CI-1', '.github/workflows', 'nightly'),
});

const preViol = (script: 'deploy' | 'deploy:preview', file = 'package.json'): Violation => ({
  rule: 'CI-1',
  file,
  line: undefined,
  message: `"${script}" has no full-suite safety net. Add "pre${script}": "npm run test:full" (npm runs it before ${script}).`,
  fingerprint: fingerprint('CI-1', file, `pre:${script}`),
});

describe('CI-03 checkCi enforces PR scoping, nightly schedules and predeploy safety nets', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z'));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('CI-03 flags every unconditional FULL_TEST form, truncates at 60 chars, strips comments, and ignores scoped and named runs', () => {
    const cmd60 = `- run: npm test ${'a'.repeat(44)}`;
    const cmd61 = `- run: npm test ${'b'.repeat(45)}`;
    const wf = '.github/workflows/pr.yml';
    const prLines = [
      'on:',
      '  pull_request_target:',
      'jobs:',
      '  t:',
      '    steps:',
      '      - run: npm test',
      '      - run: npm run test',
      '      - run: npm run -s test',
      '      - run: npm run --silent test:ci',
      '      - run: npm run test:full',
      '      - run: npm run test:all',
      '      - run: npx vitest run',
      '      - run: npx playwright test',
      '      - run: node --test',
      '      - run: "npm test"',
      "      - run: 'npm test'",
      '      - run: npm test # CANARY-ci-9e4f2a1b',
      '      - run: echo foo#bar && npm test',
      `      ${cmd60}`,
      `      ${cmd61}`,
      '# npm test CANARY-ci-9e4f2a1b',
      '      # npm test CANARY-ci-9e4f2a1b',
      '      - run: echo ok # npm test CANARY-ci-9e4f2a1b',
      '      - run: npm testx',
      '      - run: npm test:unit',
      '      - run: npx vitest runner',
      '      - run: npm run test -- --changed',
      '      - run: npx vitest run --related src/a.ts',
      '      - run: npx vitest run related src/a.ts',
      '      - run: npm test -- --only-changed',
      '      - run: npx guardrails affected && npm test',
      '      - run: npm run test:changed && npm test',
      '      - run: npm run verify:changed && npm test',
      '      - run: npx playwright test --grep smoke',
      '      - run: npm test -- --since origin/main',
      '      - run: npx playwright test --last-failed',
      '      - run: npx vitest run a.test.ts',
      '      - run: npx vitest run foo.test.ts',
      '      - run: npx vitest run --reporter=dot dir/sub.spec.tsx',
      '      - run: npx playwright test e2e/home.spec.jsx',
      '      - run: node --test test/unit.test.mjs',
      '      - run: node --test test/unit.spec.cjs',
      '      - run: npx vitest run test/unit.test.mts',
      '      - run: npx vitest run test/unit.spec.cts',
      '      - run: npx vitest run test/*.test.ts',
      '      - run: npx vitest run foo.test.xts',
      '      - run: npx vitest run foo.test.ps',
      '      - run: npx vitest run foo.unit.ts',
      '      - run: npx vitest run relatedx',
    ];
    const r = repo(
      {
        [wf]: `${prLines.join('\n')}\n`,
        '.github/workflows/nightly.yml': NIGHTLY_YML,
        'docs/notes.md': 'on:\n  pull_request:\n  schedule:\n- run: npm test\n# CANARY-ci-9e4f2a1b\n',
      },
      ON,
    );
    const out = checkCi(r.ctx());
    expect(JSON.stringify(out)).not.toContain('CANARY-ci-9e4f2a1b');
    expect(out).toEqual({
      violations: [
        prViol(wf, 6, '- run: npm test'),
        prViol(wf, 7, '- run: npm run test'),
        prViol(wf, 8, '- run: npm run -s test'),
        prViol(wf, 9, '- run: npm run --silent test:ci'),
        prViol(wf, 10, '- run: npm run test:full'),
        prViol(wf, 11, '- run: npm run test:all'),
        prViol(wf, 12, '- run: npx vitest run'),
        prViol(wf, 13, '- run: npx playwright test'),
        prViol(wf, 14, '- run: node --test'),
        prViol(wf, 15, '- run: "npm test"'),
        prViol(wf, 16, "- run: 'npm test'"),
        prViol(wf, 17, '- run: npm test'),
        prViol(wf, 18, '- run: echo foo#bar && npm test'),
        prViol(wf, 19, cmd60),
        prViol(wf, 20, cmd61),
        prViol(wf, 45, '- run: npx vitest run test/*.test.ts'),
        prViol(wf, 46, '- run: npx vitest run foo.test.xts'),
        prViol(wf, 47, '- run: npx vitest run foo.test.ps'),
        prViol(wf, 48, '- run: npx vitest run foo.unit.ts'),
        prViol(wf, 49, '- run: npx vitest run relatedx'),
      ],
    });
  });

  it('CI-03 respects all GUARDED expressions and enforces the exact 6-line lookback window', () => {
    const win6 = [
      'on:',
      '  pull_request',
      "if: github.event_name == 'schedule'",
      'name: line 1',
      'shell: bash',
      'working-directory: .',
      'env:',
      '  A: 1',
      'run: npm test',
    ].join('\n');
    const win7 = [
      'on:',
      '  pull_request',
      "if: github.event_name == 'schedule'",
      'name: line 1',
      'shell: bash',
      'working-directory: .',
      'env:',
      '  A: 1',
      '  B: 2',
      'run: npm test',
    ].join('\n');
    const r = repo(
      {
        '.github/workflows/g1.yml': "on:\n  pull_request\nif: github.event_name == 'schedule'\nrun: npm test\n# 1\n# 2\n# 3\n# 4\n# 5\n# 6\n",
        '.github/workflows/g2.yml': "on:\n  pull_request\nif: github.event_name == 'workflow_dispatch'\nrun: npm test\n",
        '.github/workflows/g3.yml': "on:\n  pull_request\nif: github.event_name != 'pull_request'\nrun: npm test\n",
        '.github/workflows/g4.yml': "on:\n  pull_request\nif: github.event_name!='pull_request'\nrun: npm test\n",
        '.github/workflows/g5.yml': "on:\n  pull_request\nif: github.event_name == 'push'\nrun: npm test\n",
        '.github/workflows/g6.yml': "on:\n  pull_request\nif: github.event_name=='push'\nrun: npm test\n",
        '.github/workflows/g7.yml': "on:\n  pull_request\nif: github.ref == 'refs/heads/main'\nrun: npm test\n",
        '.github/workflows/g-win6.yml': `${win6}\n`,
        '.github/workflows/g-win7.yml': `${win7}\n`,
        '.github/workflows/nightly.yml': NIGHTLY_YML,
      },
      ON,
    );
    expect(checkCi(r.ctx())).toEqual({
      violations: [prViol('.github/workflows/g-win7.yml', 10, 'run: npm test')],
    });
  });

  it.each<{ label: string; files: Record<string, string>; config?: object; explicitFiles?: string[]; expected: Violation[] }>([
    {
      label: 'ci.enabled false skips all checks',
      files: { '.github/workflows/ci.yml': 'on:\n  pull_request:\nsteps:\n  - run: npm test\n', 'package.json': '{"scripts":{"deploy":"x"}}' },
      config: { ...BASE_CONFIG, ci: { enabled: false } },
      expected: [],
    },
    {
      label: 'mid-line pull_request does not mark a push workflow as onPr',
      files: { '.github/workflows/push.yml': '# not pull_request\non:\n  push:\nsteps:\n  - run: npm test\n' },
      expected: [nightlyViol()],
    },
    {
      label: 'mid-line schedule: does not satisfy nightly requirement',
      files: { '.github/workflows/ci.yml': 'name: nightly schedule: check\non:\n  push:\nsteps:\n  - run: npm run test:full\n' },
      expected: [nightlyViol()],
    },
    {
      label: 'scheduled: key with non-whitespace before colon does not satisfy nightly',
      files: { '.github/workflows/ci.yml': 'on:\n  scheduled:\nsteps:\n  - run: npm run test:full\n' },
      expected: [nightlyViol()],
    },
    {
      label: 'schedule with space before colon satisfies nightly requirement',
      files: { '.github/workflows/nightly.yaml': 'on:\n  schedule :\n    - cron: "0 7 * * *"\nsteps:\n  - run: npm run test:full\n' },
      expected: [],
    },
    {
      label: 'scheduled workflow running only scoped or named tests still flags missing nightly',
      files: { '.github/workflows/nightly.yml': 'on:\n  schedule:\nsteps:\n  - run: npm test -- --changed\n  - run: npx vitest run foo.test.ts\n' },
      expected: [nightlyViol()],
    },
    {
      label: 'ci.na.nightly suppresses missing nightly violation',
      files: {},
      config: { ...BASE_CONFIG, ci: { enabled: true, na: { nightly: 'Full suite runs on every release tag.' } } },
      expected: [],
    },
    {
      label: 'custom ci.workflows globs ignore default path and scan matched files',
      files: { '.github/workflows/ignored.yml': 'on:\n  pull_request:\n  schedule:\nsteps:\n  - run: npm test\n', 'ci/pr.yaml': 'on:\n  pull_request:\nsteps:\n  - run: npm test\n' },
      config: { ...BASE_CONFIG, ci: { enabled: true, workflows: ['ci/*.yaml'] } },
      expected: [prViol('ci/pr.yaml', 4, '- run: npm test'), nightlyViol()],
    },
    {
      label: 'explicit files argument limits which workflows checkCi scans',
      files: { '.github/workflows/a.yml': 'on:\n  pull_request:\nsteps:\n  - run: npm test\n', '.github/workflows/b.yml': NIGHTLY_YML },
      explicitFiles: ['.github/workflows/a.yml'],
      expected: [prViol('.github/workflows/a.yml', 4, '- run: npm test'), nightlyViol()],
    },
  ])('CI-03 workflow selection and triggers: $label', ({ files, config, explicitFiles, expected }) => {
    const r = repo(files, config ?? ON);
    const out = explicitFiles ? checkCi(r.ctx(), explicitFiles) : checkCi(r.ctx());
    expect(out).toEqual({ violations: expected });
  });

  it.each<{ label: string; pkg?: string; pkgPath?: string; config?: object; expected: Violation[] }>([
    { label: 'missing package.json skips preDeploy', expected: [] },
    { label: 'package.json without scripts skips preDeploy', pkg: '{"name":"pkg","canary":"CANARY-ci-9e4f2a1b"}', expected: [] },
    { label: 'package.json without deploy scripts (including __proto__ key) skips preDeploy', pkg: '{"scripts":{"__proto__":"npm test","build":"tsc"}}', expected: [] },
    { label: 'missing predeploy and scoped predeploy:preview both flag', pkg: JSON.stringify({ scripts: { deploy: 'x', 'deploy:preview': 'x', 'predeploy:preview': 'npx vitest run --changed' } }), expected: [preViol('deploy'), preViol('deploy:preview')] },
    { label: 'non-full predeploy script flags deploy without leaking canary', pkg: JSON.stringify({ scripts: { deploy: 'x', predeploy: 'echo CANARY-ci-9e4f2a1b' } }), expected: [preViol('deploy')] },
    { label: 'full-suite predeploy and predeploy:preview pass', pkg: JSON.stringify({ scripts: { deploy: 'x', predeploy: 'npm run -s test:full', 'deploy:preview': 'x', 'predeploy:preview': 'npx vitest run' } }), expected: [] },
    { label: 'runbook.na.deploy skips only deploy', pkg: JSON.stringify({ scripts: { deploy: 'x', 'deploy:preview': 'x' } }), config: { ...ON, runbook: { na: { deploy: 'Library released via tag workflow.' } } }, expected: [preViol('deploy:preview')] },
    { label: 'runbook.na deploy:preview skips only deploy:preview', pkg: JSON.stringify({ scripts: { deploy: 'x', 'deploy:preview': 'x' } }), config: { ...ON, runbook: { na: { 'deploy:preview': 'No preview environment exists.' } } }, expected: [preViol('deploy')] },
    { label: 'ci.na.preDeploy skips both deploy and deploy:preview', pkg: JSON.stringify({ scripts: { deploy: 'x', 'deploy:preview': 'x' } }), config: { ...BASE_CONFIG, ci: { enabled: true, na: { preDeploy: 'CI deploy job always runs full suite first.' } } }, expected: [] },
    { label: 'custom runbook.packageJson path is read and reported', pkg: JSON.stringify({ scripts: { deploy: 'x' } }), pkgPath: 'apps/web/package.json', config: { ...ON, runbook: { packageJson: 'apps/web/package.json' } }, expected: [preViol('deploy', 'apps/web/package.json')] },
  ])('CI-03 preDeploy safety nets: $label', ({ pkg, pkgPath, config, expected }) => {
    const files: Record<string, string> = { '.github/workflows/nightly.yml': NIGHTLY_YML };
    if (pkg !== undefined) files[pkgPath ?? 'package.json'] = pkg;
    const out = checkCi(repo(files, config ?? ON).ctx());
    expect(JSON.stringify(out)).not.toContain('CANARY-ci-9e4f2a1b');
    expect(out).toEqual({ violations: expected });
  });

  it('CI-03 throws SyntaxError on malformed package.json', () => {
    const r = repo({ '.github/workflows/nightly.yml': NIGHTLY_YML, 'package.json': '{not-json' }, ON);
    expect(() => checkCi(r.ctx())).toThrow(SyntaxError);
  });
});
