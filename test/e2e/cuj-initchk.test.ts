import { join } from 'node:path';
import { expect, test } from 'vitest';
import { checkJson, e2eRepo, runCli } from '../helpers/e2e-cli.js';

test('CUJ-INITCHK developer scaffolds a fresh git repo with guardrails init and the first check passes', () => {
  const r = e2eRepo(
    {
      'package.json': '{"name":"fresh-app","version":"1.0.0"}\n',
      'src/main.ts': 'export const appName = "fresh-app";\n',
      'AGENTS.md': '# Custom project instructions\n',
    },
    null,
  ).commit('initial');

  expect(() => runCli(r.dir, ['check'], { cliPath: join(r.dir, 'missing-dist-cli.js') })).toThrow(
    /Built guardrails CLI not found.*Run "npm run build"/,
  );

  const beforeInit = checkJson(r.dir);
  expect([
    beforeInit.status,
    beforeInit.report.ok,
    beforeInit.report.findings.map((f) => [f.rule, f.mode, f.file, f.message]),
  ]).toEqual([
    1,
    false,
    [['CFG-1', 'block', 'guardrails.config.json', `No guardrails.config.json in ${r.dir}. Run "guardrails init" first.`]],
  ]);

  const firstInit = runCli(r.dir, ['init', '--owner', '@ws42-admin', '--base', 'main']);
  expect(firstInit.status).toBe(0);
  expect(firstInit.stdout).toContain('created guardrails.config.json');
  expect(firstInit.stdout).toContain('merged  AGENTS.md');
  expect(r.read('AGENTS.md').startsWith('# Custom project instructions\n')).toBe(true);
  expect(r.read('AGENTS.md')).toContain('<!-- guardrails:start -->');
  expect(r.read('GEMINI.md')).toBe('See AGENTS.md.\n');
  expect(r.read('CLAUDE.md')).toBe('See AGENTS.md.\n');
  expect(r.read('.github/CODEOWNERS')).toContain('/guardrails.config.json        @ws42-admin');
  expect(r.exists('.github/workflows/guardrails.yml')).toBe(true);
  expect(r.exists('.claude/settings.json')).toBe(true);
  expect(r.exists('.codex/config.toml')).toBe(true);

  const secondInit = runCli(r.dir, ['init', '--owner', '@other']);
  expect(secondInit.status).toBe(0);
  expect(secondInit.stdout).not.toMatch(/^(created|merged) /m);
  expect(secondInit.stdout).toContain('skipped guardrails.config.json (exists, nothing to add; --force to overwrite)');
  expect(r.read('.github/CODEOWNERS')).toContain('@ws42-admin');

  const { status, report } = checkJson(r.dir);
  expect([status, report.ok, report.counts, report.findings]).toEqual([
    0,
    true,
    { block: 0, warn: 0, known: 0 },
    [],
  ]);
  const cfg = JSON.parse(r.read('guardrails.config.json')) as { runbook?: { enabled?: boolean }; ci?: { enabled?: boolean } };
  expect([cfg.runbook?.enabled ?? false, cfg.ci?.enabled ?? false]).toEqual([false, false]);
});
