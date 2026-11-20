import { join } from 'node:path';
import { expect, test } from 'vitest';
import { e2eRepo, runCli } from '../helpers/e2e-cli.js';

const LOCK_WHY = 'guardrails config, baseline, flags, CODEOWNERS, CI and hook settings are locked';
const SPEC_WHY = 'existing spec-tagged test files are locked during builds';
const HOOK_TIP = 'If the test or spec is wrong, STOP and call guardrails_flag. Humans can unlock with GUARDRAILS_ALLOW_TEST_EDITS=1.';

test('CUJ-HOOK claude hook-guard denies editing guardrails.config.json and allows when unlocked', () => {
  const r = e2eRepo({
    'src/core/a.test.ts': "import { expect, it } from 'vitest';\nit('CORE-01 adds', () => { expect(1 + 1).toBe(2); });\n",
    'src/core/add.ts': 'export const add = (a: number, b: number): number => a + b;\n',
  });

  const configEditPayload = JSON.stringify({
    tool_name: 'Edit',
    tool_input: { file_path: join(r.dir, 'guardrails.config.json') },
  });

  const denied = runCli(r.dir, ['hook-guard', '--agent', 'claude'], {
    input: configEditPayload,
    env: { CLAUDE_PROJECT_DIR: r.dir },
  });
  expect([denied.status, denied.stdout, denied.stderr]).toEqual([
    2,
    '',
    `guardrails: guardrails.config.json is protected (${LOCK_WHY}). ${HOOK_TIP}\n`,
  ]);

  const canary = 'CANARY-HOOK-SECRET-9f3a42';
  const originalConfig = r.read('guardrails.config.json');
  const shellDenied = runCli(r.dir, ['hook-guard', '--agent', 'claude'], {
    input: JSON.stringify({
      tool_name: 'Bash',
      tool_input: { command: `echo "${canary}" > guardrails.config.json` },
    }),
    env: { CLAUDE_PROJECT_DIR: r.dir },
  });
  expect([shellDenied.status, shellDenied.stdout, shellDenied.stderr]).toEqual([
    2,
    '',
    `guardrails: this command writes to guardrails.config.json (${LOCK_WHY}). ${HOOK_TIP}\n`,
  ]);
  expect(`${shellDenied.stdout}${shellDenied.stderr}`).not.toContain(canary);
  expect(r.read('guardrails.config.json')).toBe(originalConfig);

  const traversalDenied = runCli(r.dir, ['hook-guard', '--agent', 'claude'], {
    input: JSON.stringify({
      tool_name: 'Edit',
      tool_input: { file_path: join(r.dir, 'src', 'core', '..', '..', 'guardrails.config.json') },
    }),
    env: { CLAUDE_PROJECT_DIR: r.dir },
  });
  expect([traversalDenied.status, traversalDenied.stdout, traversalDenied.stderr]).toEqual([
    2,
    '',
    `guardrails: guardrails.config.json is protected (${LOCK_WHY}). ${HOOK_TIP}\n`,
  ]);

  const specTestDenied = runCli(r.dir, ['hook-guard', '--agent', 'claude'], {
    input: JSON.stringify({
      tool_name: 'Edit',
      tool_input: { file_path: join(r.dir, 'src/core/a.test.ts') },
    }),
    env: { CLAUDE_PROJECT_DIR: r.dir },
  });
  expect([specTestDenied.status, specTestDenied.stdout, specTestDenied.stderr]).toEqual([
    2,
    '',
    `guardrails: src/core/a.test.ts is protected (${SPEC_WHY}). ${HOOK_TIP}\n`,
  ]);

  const selfUnlockDenied = runCli(r.dir, ['hook-guard', '--agent', 'claude'], {
    input: JSON.stringify({
      tool_name: 'Bash',
      tool_input: { command: 'GUARDRAILS_ALLOW_TEST_EDITS=1 npx guardrails journeys list' },
    }),
    env: { CLAUDE_PROJECT_DIR: r.dir },
  });
  expect([selfUnlockDenied.status, selfUnlockDenied.stdout, selfUnlockDenied.stderr]).toEqual([
    2,
    '',
    `guardrails: agents may not add, approve or unlock journeys, tests or guardrails. ${HOOK_TIP}\n`,
  ]);

  const malformedStdin = runCli(r.dir, ['hook-guard', '--agent', 'claude'], {
    input: `{malformed\u0000${canary}`,
    env: { CLAUDE_PROJECT_DIR: r.dir },
  });
  expect([malformedStdin.status, malformedStdin.stdout, malformedStdin.stderr]).toEqual([0, '', '']);

  const unlockedProduct = runCli(r.dir, ['hook-guard', '--agent', 'claude'], {
    input: JSON.stringify({
      tool_name: 'Edit',
      tool_input: { file_path: join(r.dir, 'src/core/add.ts') },
    }),
    env: { CLAUDE_PROJECT_DIR: r.dir },
  });
  expect([unlockedProduct.status, unlockedProduct.stdout, unlockedProduct.stderr]).toEqual([0, '', '']);

  const allowed = runCli(r.dir, ['hook-guard', '--agent', 'claude'], {
    input: configEditPayload,
    env: { CLAUDE_PROJECT_DIR: r.dir, GUARDRAILS_ALLOW_TEST_EDITS: '1' },
  });
  expect([allowed.status, allowed.stdout, allowed.stderr]).toEqual([0, '', '']);
});
