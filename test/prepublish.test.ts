import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { describe, expect, it } from 'vitest';
import { guard, hookResponse } from '../src/hook.js';
import { init, initRunbook, mergeCodexToml } from '../src/init.js';
import { draftJourneys } from '../src/journeys.js';
import { createServer } from '../src/mcp.js';
import { run } from '../src/runner.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const FRESH = ['guardrails.config.json', 'AGENTS.md', '.github/CODEOWNERS', 'vitest.config.ts', 'vitest.setup.ts'];
const fresh = () => {
  const r = repo({ 'package.json': '{"name":"x","version":"1.0.0"}\n', 'src/main.ts': 'export const a = 1;\n' });
  r.remove(...FRESH).commit();
  return r;
};

describe('INIT-02 a fresh init followed by check passes without enabling standards init does not scaffold', () => {
  it('has no blocking findings, and init runbook is the opt-in that turns the runbook and CI standards on', async () => {
    const r = fresh();
    init({ cwd: r.dir, owner: '@me' });
    r.commit();
    const before = await run({ cwd: r.dir, knip: false });
    expect([before.ok, before.counts.block, before.findings.filter((f) => /^(RUN|CI)-/.test(f.rule))]).toEqual([true, 0, []]);
    initRunbook({ cwd: r.dir });
    const cfg = JSON.parse(readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8'));
    expect([cfg.runbook.enabled, cfg.ci.enabled]).toEqual([true, true]);
  });
});

describe('INIT-03 init sets up Codex CLI: MCP server, agent marker and a guard hook', () => {
  it('writes the Codex files, locks them, and appends to an existing config.toml without duplicating or rewriting it', () => {
    const r = fresh();
    const res = init({ cwd: r.dir });
    expect(res.written).toEqual(expect.arrayContaining(['.codex/hooks.json', '.codex/config.toml']));
    const toml = readFileSync(join(r.dir, '.codex/config.toml'), 'utf8');
    expect(toml).toContain('[mcp_servers.guardrails]\ncommand = "npx"\nargs = ["--no-install", "guardrails-mcp"]');
    expect(toml).toContain('[shell_environment_policy.set]\nGUARDRAILS_AGENT = "codex"');
    const hook = JSON.parse(readFileSync(join(r.dir, '.codex/hooks.json'), 'utf8')).hooks.PreToolUse[0];
    expect([hook.matcher, hook.hooks[0].command]).toEqual(['Bash|apply_patch', 'npx --no-install guardrails hook-guard --agent codex']);
    expect(init({ cwd: r.dir }).written).toEqual([]);

    const mine = 'model = "o"\n\n[mcp_servers.other]\ncommand = "x"\n';
    const merged = mergeCodexToml(mine, toml) ?? '';
    expect([merged.startsWith(mine), merged.match(/\[mcp_servers\.guardrails\]/g)?.length, merged.match(/\[shell_environment_policy\.set\]/g)?.length]).toEqual([true, 1, 1]);
    expect(mergeCodexToml(`${mine}\n[mcp_servers.guardrails]\ncommand = "mine"\n`, toml)).not.toContain('guardrails-mcp');
    expect(mergeCodexToml('[shell_environment_policy]\ninherit = "core"\n', toml)).not.toContain('GUARDRAILS_AGENT');
    expect(mergeCodexToml(toml, toml)).toBeNull();
  });

  it('keeps a user config.toml and adds only the missing tables', () => {
    const r = fresh();
    r.write({ '.codex/config.toml': 'model = "o"\n' });
    expect(init({ cwd: r.dir }).written).toContain('.codex/config.toml');
    const out = readFileSync(join(r.dir, '.codex/config.toml'), 'utf8');
    expect([out.startsWith('model = "o"\n'), out.match(/\[mcp_servers\.guardrails\]/g)?.length, out.match(/\[shell_environment_policy\.set\]/g)?.length]).toEqual([true, 1, 1]);
  });
});

describe('LOCK-02 the Codex hook denies apply_patch and shell writes to locked files', () => {
  const project = () => repo({ 'src/core/a.test.ts': "it('CORE-01 adds', () => { expect(1).toBe(1) });\n", 'src/core/plain.test.ts': "it('x', () => {});\n", 'src/app.ts': 'x\n' }).dir;
  const patch = (...lines: string[]) => `*** Begin Patch\n${lines.join('\n')}\n*** End Patch`;
  it.each([
    [patch('*** Update File: src/core/a.test.ts', '@@', '-expect(1)', '+expect(2)'), 'src/core/a.test.ts'],
    [patch('*** Update File: src/app.ts', '@@', '-x', '+y', '*** Delete File: guardrails.config.json'), 'guardrails.config.json'],
    [patch('*** Add File: .codex/hooks.json', '+{}'), '.codex/hooks.json'],
    [patch('*** Update File: src/app.ts', '*** Move to: .github/workflows/ci.yml', '@@', '-x', '+y'), '.github/workflows/ci.yml'],
    [patch('*** Update File: src/app.ts', '@@', '-x', '+guardrails.config.json > nope'), null],
    [patch('*** Add File: src/core/new.test.ts', "+it('CORE-01 new', () => {});"), null],
    [patch('*** Update File: src/core/plain.test.ts', '@@', '-x', '+y'), null],
  ])('apply_patch %#', (command, blocked) => {
    const dir = project();
    const r = guard({ tool_name: 'apply_patch', tool_input: { command }, cwd: dir }, {});
    expect([r.allow, r.path ?? null]).toEqual([blocked === null, blocked]);
  });

  it('guards shell writes, resolves paths from a subfolder, and answers with exit 2 plus the reason on stderr', () => {
    const dir = project();
    expect(guard({ tool_name: 'Bash', tool_input: { command: "sed -i 's/1/2/' src/core/a.test.ts" }, cwd: dir }, {}).path).toBe('src/core/a.test.ts');
    expect(guard({ tool_name: 'Bash', tool_input: { command: 'cat src/core/a.test.ts' }, cwd: dir }, {}).allow).toBe(true);
    expect(guard({ tool_name: 'Bash', tool_input: { command: 'npx guardrails journeys approve CUJ-X' }, cwd: dir }, {}).allow).toBe(false);
    const sub = join(dir, 'src/core');
    expect(guard({ tool_name: 'apply_patch', tool_input: { command: patch('*** Update File: a.test.ts') }, cwd: sub }, {}).path).toBe('src/core/a.test.ts');
    expect(guard({ tool_name: 'Bash', tool_input: { command: 'echo x > ../../guardrails.config.json' }, cwd: sub }, {}).path).toBe('guardrails.config.json');
    const denied = hookResponse('codex', guard({ tool_name: 'apply_patch', tool_input: { command: patch('*** Update File: guardrails.config.json') }, cwd: dir }, {}));
    expect([denied.code, denied.stdout, denied.stderr]).toEqual([2, '', expect.stringContaining('guardrails.config.json is protected')]);
    expect(hookResponse('codex', { allow: true })).toEqual({ code: 0, stdout: '', stderr: '' });
  });
});

describe('JOURNEY-06 journeys draft --dry-run never calls a model', () => {
  const site = () => repo({ 'src/pages/contact.tsx': 'x', 'README.md': '# Bakery site' }, { ...BASE_CONFIG, sourceRoots: ['src'] });
  it('skips Gemini and OpenAI even when both keys are set, and still previews the heuristic drafts', async () => {
    const r = site();
    const calls: string[] = [];
    const spy = async (url: string) => { calls.push(url); throw new Error('network used'); };
    const res = await draftJourneys(r.dir, { env: { GEMINI_API_KEY: 'k', OPENAI_API_KEY: 'k' }, fetch: spy, dryRun: true });
    expect([calls, res.via, Object.keys(res.drafts)]).toEqual([[], 'heuristic preview (--dry-run makes no model call)', ['CUJ-CONTACT']]);
    await expect(draftJourneys(r.dir, { env: { GEMINI_API_KEY: 'k' }, fetch: spy })).rejects.toThrow('network used');
    expect(calls).toHaveLength(1);
  });

  it('prints the preview from the CLI without writing the config', () => {
    const r = site();
    const before = readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8');
    const out = spawnSync(process.execPath, [resolve('dist/cli.js'), 'journeys', 'draft', '--dry-run', '--cwd', r.dir], { encoding: 'utf8', env: { ...process.env, GEMINI_API_KEY: 'unused', OPENAI_API_KEY: 'unused', GUARDRAILS_AGENT: '' } });
    expect([out.status, JSON.parse(out.stdout).via, readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8')]).toEqual([0, 'heuristic preview (--dry-run makes no model call)', before]);
  });
});

describe('MCP-02 the MCP server reports the package version', () => {
  it('matches package.json instead of a hardcoded number', async () => {
    const [a, b] = InMemoryTransport.createLinkedPair();
    const client = new Client({ name: 't', version: '0' });
    await Promise.all([createServer().connect(a), client.connect(b)]);
    const { version } = JSON.parse(readFileSync('package.json', 'utf8'));
    expect(client.getServerVersion()).toMatchObject({ name: 'guardrails', version });
    await client.close();
  });
});
