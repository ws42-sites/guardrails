import { expect, test } from 'vitest';
import {
  E2E_BASE_CONFIG,
  VITEST_CMD,
  VITEST_RUNNER_FILE,
  checkJson,
  e2eRepo,
  e2eVitestConfigSource,
  vitestRunnerSource,
} from '../helpers/e2e-cli.js';

test('CUJ-FAILBASE new test passing on base is blocked while failing on base and passing on head passes', () => {
  const r = e2eRepo(
    {
      '.gitignore': '.vite/\nnode_modules/\n',
      'package.json': '{"name":"e2e-fixture","private":true,"type":"module"}\n',
      'vitest.config.ts': e2eVitestConfigSource(),
      [VITEST_RUNNER_FILE]: vitestRunnerSource(),
      'src/core/add.ts': 'export const add = (a: number, b: number): number => a - b;\n',
      'src/core/base.test.ts':
        "import { expect, it } from 'vitest';\nimport { add } from './add.js';\nit('CORE-01 subtracts zero', () => { expect(add(5, 0)).toBe(5); });\n",
      'src/lib/fmt.test.ts':
        "import { expect, it } from 'vitest';\nit('LIB-01 formats string', () => { expect('x').toBe('x'); });\n",
    },
    { ...E2E_BASE_CONFIG, tests: { command: VITEST_CMD } },
  )
    .commit('buggy base')
    .branch('fix/addition');

  r.write({
    'src/core/add.ts': 'export const add = (a: number, b: number): number => a + b;\n',
    'src/core/new.test.ts': [
      "import { expect, it } from 'vitest';",
      "import { add } from './add.js';",
      "it('CORE-01 adds zeroes', () => { expect(add(0, 0)).toBe(0); });",
      "it('CORE-01 broken head assertion', () => { expect(add(2, 3)).toBe(999); });",
      '',
    ].join('\n'),
  });

  const passesOnBase = checkJson(r.dir, ['--base', 'main', '--fails-on-base']);
  expect([
    passesOnBase.status,
    passesOnBase.report.ok,
    passesOnBase.report.findings.filter((f) => f.mode === 'block').map((f) => [f.rule, f.file, f.line, f.message]),
  ]).toEqual([
    1,
    false,
    [
      [
        'TEST-11',
        'src/core/new.test.ts',
        3,
        '"CORE-01 adds zeroes" already passes on main without this change, so it does not test the change. Assert the new behaviour, or mark CORE-01 regressionGuard / have a code owner add the "guardrails:existing-behaviour-tests-approved" label, which approves new tests that lock in existing behaviour.',
      ],
      ['TEST-11', 'src/core/new.test.ts', 4, '"CORE-01 broken head assertion" does not pass on this branch.'],
    ],
  ]);

  r.write({
    'src/core/new.test.ts':
      "import { expect, it } from 'vitest';\nimport { add } from './add.js';\nit('CORE-01 adds two numbers', () => { expect(add(2, 3)).toBe(5); });\n",
  });

  const accepted = checkJson(r.dir, ['--base', 'main', '--fails-on-base']);
  expect([
    accepted.status,
    accepted.report.ok,
    accepted.report.counts.block,
    accepted.report.findings.filter((f) => f.mode === 'block'),
    accepted.report.metrics.find((m) => m.name === 'failsOnBase')?.value,
  ]).toEqual([0, true, 0, [], '1/1']);
});
