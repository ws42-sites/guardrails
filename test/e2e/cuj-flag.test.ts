import { expect, test } from 'vitest';
import { checkJson, e2eRepo, runCli } from '../helpers/e2e-cli.js';

test('CUJ-FLAG agent flags a spec conflict so check blocks until a human empties flags.json', () => {
  const r = e2eRepo({
    'src/core/a.test.ts': "import { expect, it } from 'vitest';\nit('CORE-01 adds', () => { expect(2 + 2).toBe(4); });\n",
    'src/lib/b.test.ts': "import { expect, it } from 'vitest';\nit('LIB-01 formats', () => { expect('a').toBe('a'); });\n",
  }).commit('clean');

  const shortReason = runCli(r.dir, ['flag', 'CORE-01', 'short', '--file', 'src/core/a.test.ts']);
  expect([shortReason.status, shortReason.stderr, r.exists('.guardrails/flags.json')]).toEqual([
    2,
    'reason must explain the conflict (at least 10 characters)\n',
    false,
  ]);

  const canary = 'CANARY-FLAG-SECRET-7b2c91';
  r.write({ '.guardrails/flags.json': `{corrupt ${canary}` });
  const corrupt = checkJson(r.dir);
  expect([
    corrupt.status,
    corrupt.report.findings.map((f) => [f.rule, f.mode, f.file, f.message]),
    JSON.stringify(corrupt).includes(canary),
  ]).toEqual([
    1,
    [['FLAG-1', 'block', '.guardrails/flags.json', 'is not valid JSON. A human must repair or empty it.']],
    false,
  ]);
  r.remove('.guardrails/flags.json');

  const flagged = runCli(
    r.dir,
    ['flag', 'CORE-01', 'test expects 5 for add(2,2) which contradicts spec', '--file', 'src/core/a.test.ts'],
    { env: { GUARDRAILS_AGENT: 'claude-code' } },
  );
  expect([flagged.status, flagged.stdout]).toEqual([
    0,
    'Flagged F1 on CORE-01 in .guardrails/flags.json. STOP work on this spec; a human must resolve it (merge is blocked by FLAG-1).\n',
  ]);

  const blocked = checkJson(r.dir);
  expect([
    blocked.status,
    blocked.report.ok,
    blocked.report.findings.map((f) => [f.rule, f.mode, f.file, f.message]),
  ]).toEqual([
    1,
    false,
    [
      [
        'FLAG-1',
        'block',
        'src/core/a.test.ts',
        'open flag F1 on CORE-01: test expects 5 for add(2,2) which contradicts spec. A human must resolve it before merge.',
      ],
    ],
  ]);

  r.write({ '.guardrails/flags.json': '{"version":1,"flags":[]}\n' });
  const cleared = checkJson(r.dir);
  expect([cleared.status, cleared.report.ok, cleared.report.counts, cleared.report.findings]).toEqual([
    0,
    true,
    { block: 0, warn: 0, known: 0 },
    [],
  ]);
});
