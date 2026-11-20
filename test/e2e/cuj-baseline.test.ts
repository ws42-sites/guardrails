import { expect, test } from 'vitest';
import { E2E_BASE_CONFIG, checkJson, e2eRepo, runCli } from '../helpers/e2e-cli.js';

const RUNNER_CHECK = 'export const isRunner = Boolean(process.env.VITEST);\n';

test('CUJ-BASELINE baseline records known violations, shrinks on fix, and blocks growth vs base', () => {
  const r = e2eRepo(
    {
      'src/core/a.test.ts':
        "import { expect, it } from 'vitest';\nit('CORE-01 adds', () => { expect(1 + 1).toBe(2); });\nit('LIB-01 formats', () => { expect('x').toBe('x'); });\n",
      'src/a.ts': RUNNER_CHECK,
      'src/b.ts': RUNNER_CHECK,
    },
    { ...E2E_BASE_CONFIG, testSuite: { enabled: true, maxLines: 10 } },
  );

  expect(runCli(r.dir, ['check-tests', '--update-baseline']).status).toBe(0);

  const initialRecord = runCli(r.dir, ['baseline']);
  expect([initialRecord.status, initialRecord.stdout]).toEqual([
    0,
    '.guardrails/baseline.json: 2 entries (0 removed, 2 added) [initial]\n',
  ]);

  const withBaseline = checkJson(r.dir);
  expect([withBaseline.status, withBaseline.report.ok, withBaseline.report.counts]).toEqual([
    0,
    true,
    { block: 0, warn: 0, known: 2 },
  ]);

  r.remove('src/a.ts').write({ 'src/unbaselined.ts': RUNNER_CHECK });
  const ratcheted = runCli(r.dir, ['baseline']);
  expect([ratcheted.status, ratcheted.stdout]).toEqual([
    0,
    '.guardrails/baseline.json: 1 entries (1 removed, 0 added)\n',
  ]);
  const saved = JSON.parse(r.read('.guardrails/baseline.json')) as { entries: { rule: string; file: string }[] };
  expect(saved.entries.map((e) => [e.rule, e.file])).toEqual([['ARCH-7', 'src/b.ts']]);
  r.remove('src/unbaselined.ts');

  r.commit('ratcheted baselines').branch('feat/grow-baselines');

  const grownEntries = [...saved.entries, { rule: 'DOC-1', file: 'NOTES.md', fingerprint: 'abcdef012345' }];
  r.write({
    '.guardrails/baseline.json': `${JSON.stringify({ version: 1, entries: grownEntries }, null, 2)}\n`,
    'src/core/oversize.test.ts': [
      "import { expect, it } from 'vitest';",
      "it('CORE-01 case one', () => {",
      '  const a = 1;',
      '  const b = 2;',
      '  expect(a + b).toBe(3);',
      '});',
      "it('CORE-01 case two', () => {",
      '  const c = 4;',
      '  const d = 5;',
      '  expect(c + d).toBe(9);',
      '});',
      '',
    ].join('\n'),
  });
  const suiteBaseline = JSON.parse(r.read('.guardrails/test-suite-baseline.json')) as {
    version: 1;
    maxLines: number;
    totals: object;
    byKind: Record<string, number>;
    counts: Record<string, number>;
    oversize: Record<string, number>;
  };
  r.write({
    '.guardrails/test-suite-baseline.json': `${JSON.stringify(
      {
        ...suiteBaseline,
        byKind: { ...suiteBaseline.byKind, oversize: 1 },
        oversize: { 'src/core/oversize.test.ts': 11 },
      },
      null,
      2,
    )}\n`,
  });

  const grownCheck = checkJson(r.dir, ['--base', 'main']);
  expect([
    grownCheck.status,
    grownCheck.report.ok,
    grownCheck.report.counts,
    grownCheck.report.findings
      .filter((f) => f.mode === 'block' && !f.known)
      .map((f) => [f.rule, f.file, f.message])
      .sort(([a], [b]) => a.localeCompare(b)),
  ]).toEqual([
    1,
    false,
    { block: 2, warn: 0, known: 1 },
    [
      [
        'BASE-1',
        '.guardrails/baseline.json',
        'baseline grew by 1 entry. Fix the new violations instead, or have a code owner add the "guardrails:baseline-approved" label, which approves letting the baseline grow.',
      ],
      [
        'SUITE-6',
        '.guardrails/test-suite-baseline.json',
        'test-suite baseline grew (oversize 0 -> 1). Fix the new findings instead, or have a code owner add the "guardrails:baseline-approved" label, which approves letting the test-suite baseline grow.',
      ],
    ],
  ]);
});
