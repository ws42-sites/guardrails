import { expect, test } from 'vitest';
import { checkJson, e2eRepo } from '../helpers/e2e-cli.js';

const BASE_ADD_TEST = [
  "import { expect, it } from 'vitest';",
  "import { add } from './add.js';",
  "it('CORE-01 adds positives', () => { expect(add(1, 2)).toBe(3); });",
  "it('CORE-01 adds negatives and zeroes', () => {",
  '  expect(add(-2, 2)).toBe(0);',
  '  expect(add(0, 0)).toBe(0);',
  '});',
  '',
].join('\n');

test('CUJ-NOWEAKEN branch deleting a base test or removing an expect blocks unless labelled', () => {
  const r = e2eRepo({
    'src/core/add.ts': 'export const add = (a: number, b: number): number => a + b;\n',
    'src/core/add.test.ts': BASE_ADD_TEST,
    'src/lib/fmt.test.ts': "import { expect, it } from 'vitest';\nit('LIB-01 formats string', () => { expect('ok').toBe('ok'); });\n",
  })
    .commit('base suite')
    .branch('feat/weaken');

  r.write({
    'src/core/add.test.ts': [
      "import { expect, it } from 'vitest';",
      "import { add } from './add.js';",
      "it('CORE-01 adds positives', () => { expect(add(1, 2)).toBeTruthy(); });",
      "it('CORE-01 adds negatives and zeroes', () => {",
      '  expect(add(-2, 2)).toBe(0);',
      '});',
      '',
    ].join('\n'),
  });

  const weakened = checkJson(r.dir, ['--base', 'main'], {
    GUARDRAILS_LABELS: 'guardrails:tests-removed-approved',
  });
  expect([
    weakened.status,
    weakened.report.ok,
    weakened.report.findings.filter((f) => f.mode === 'block').map((f) => [f.rule, f.file, f.line, f.message]),
  ]).toEqual([
    1,
    false,
    [
      ['TEST-7', 'src/core/add.test.ts', 3, '"CORE-01 adds positives" loosened a matcher (toBe -> toBeTruthy). Keep the stricter matcher.'],
      ['TEST-7', 'src/core/add.test.ts', 4, '"CORE-01 adds negatives and zeroes" lost 1 expect() call(s). Restore them.'],
    ],
  ]);

  r.write({
    'src/core/add.test.ts': [
      "import { expect, it } from 'vitest';",
      "import { add } from './add.js';",
      "it('CORE-01 adds positives', () => { expect(add(1, 2)).toBe(3); });",
      '',
    ].join('\n'),
  });

  const deletedUnlabelled = checkJson(r.dir, ['--base', 'main']);
  expect([
    deletedUnlabelled.status,
    deletedUnlabelled.report.ok,
    deletedUnlabelled.report.findings.map((f) => [f.rule, f.mode, f.file, f.message]),
  ]).toEqual([
    1,
    false,
    [
      ['TEST-10', 'warn', '(diff)', 'test count dropped by 1 (3 on base, 2 now).'],
      [
        'TEST-7',
        'block',
        'src/core/add.test.ts',
        'deleted base test "CORE-01 adds negatives and zeroes". Deleting base tests needs a code owner to add the "guardrails:tests-removed-approved" label, which approves removing tests.',
      ],
    ],
  ]);

  const deletedApproved = checkJson(r.dir, ['--base', 'main'], {
    GUARDRAILS_LABELS: 'guardrails:tests-removed-approved',
  });
  expect([
    deletedApproved.status,
    deletedApproved.report.ok,
    deletedApproved.report.counts,
    deletedApproved.report.findings.map((f) => [f.rule, f.mode, f.message]),
  ]).toEqual([
    0,
    true,
    { block: 0, warn: 1, known: 0 },
    [['TEST-10', 'warn', 'test count dropped by 1 (3 on base, 2 now).']],
  ]);
});
