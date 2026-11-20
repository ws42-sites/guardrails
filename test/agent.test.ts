import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { describe, expect, it } from 'vitest';
import { createServer } from '../src/mcp.js';
import { init } from '../src/init.js';
import { parseConfig } from '../src/config.js';
import { validateTest } from '../src/validate.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

describe('INIT-01 init scaffolds a repo without clobbering it', () => {
  it('writes config, AGENTS.md pointers, CODEOWNERS and CI, guessing tiers, and merges into existing agent docs', () => {
    const r = repo({ 'src/lib/storage/x.ts': '', 'functions/api/a.ts': '', 'src/main.tsx': '', '.claude/settings.json': '{"model":"x","hooks":{"PreToolUse":[]}}' });
    r.remove('guardrails.config.json', '.github/CODEOWNERS');
    const res = init({ cwd: r.dir, owner: '@josh', base: 'main' });
    expect([res.written, res.skipped]).toEqual([
      ['guardrails.config.json', 'AGENTS.md', 'GEMINI.md', 'CLAUDE.md', '.github/CODEOWNERS', '.github/workflows/guardrails.yml', '.claude/settings.json', '.mcp.json', '.gemini/settings.json', '.agents/hooks.json', '.agents/mcp_config.json', '.codex/hooks.json', '.codex/config.toml', '.gemini/.env'],
      [],
    ]);
    const cfg = parseConfig(JSON.parse(readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8')));
    expect([cfg.sourceRoots, cfg.tiers.T1, cfg.architecture.entries]).toEqual([['src', 'functions'], ['functions/api/**', 'src/lib/storage/**'], ['src/main.tsx', 'functions/**/*.ts']]);
    expect(readFileSync(join(r.dir, '.github/CODEOWNERS'), 'utf8')).toContain('/guardrails.config.json        @josh');
    expect(readFileSync(join(r.dir, 'GEMINI.md'), 'utf8')).toBe('See AGENTS.md.\n');
    expect(existsSync(join(r.dir, '.github/workflows/guardrails.yml'))).toBe(true);
    const claude = JSON.parse(readFileSync(join(r.dir, '.claude/settings.json'), 'utf8'));
    expect([claude.model, claude.env.GUARDRAILS_AGENT, claude.hooks.PreToolUse[0].hooks[0].command]).toEqual(['x', 'claude-code', 'npx --no-install guardrails hook-guard --agent claude']);
    expect(init({ cwd: r.dir }).written).toEqual([]);
  });
});

describe('MCP-01 agents can validate a test before writing it', () => {
  it('reports unknown IDs, the per-ID cap and the file glob', () => {
    const r = repo({ 'src/core/a.test.ts': `it('CORE-01 a', () => { expect(1).toBe(1) });\nit('CORE-01 b', () => { expect(2).toBe(2) });\n` },
      { ...BASE_CONFIG, specs: { ...BASE_CONFIG.specs, maxTestsPerId: 2 } });
    expect(validateTest({ cwd: r.dir, specId: 'NOPE-01' }).problems).toEqual([
      'NOPE-01 is not in guardrails.config.json specs.items. Ask a code owner to add the behaviour first, or reuse an existing ID.',
    ]);
    const full = validateTest({ cwd: r.dir, specId: 'CORE-01', testFile: 'src/core/a.ts' });
    expect([full.ok, full.problems.length, full.spec?.existingTests]).toEqual([false, 2, 2]);
    expect(validateTest({ cwd: r.dir, specId: 'LIB-01', testFile: 'src/lib/f.test.ts' })).toMatchObject({ ok: true, problems: [], spec: { tier: 'T2', existingTests: 0 } });
  });

  it('exposes the tools over MCP, including stop-and-flag', async () => {
    const r = repo({ 'src/core/a.test.ts': `it('CORE-01 a', () => { expect(1).toBe(1) });\n` });
    const [a, b] = InMemoryTransport.createLinkedPair();
    const client = new Client({ name: 't', version: '0' });
    await Promise.all([createServer().connect(a), client.connect(b)]);
    const tools = (await client.listTools()).tools.map((t) => t.name).sort();
    expect(tools).toEqual(['guardrails_check', 'guardrails_explain_rule', 'guardrails_flag', 'guardrails_journey_coverage', 'guardrails_list_journeys', 'guardrails_list_specs', 'guardrails_mutants', 'guardrails_report', 'guardrails_validate_test']);
    const res = await client.callTool({ name: 'guardrails_list_specs', arguments: { cwd: r.dir, tier: 'T1' } });
    expect(JSON.parse((res.content as { text: string }[])[0].text)).toEqual([{ id: 'CORE-01', tier: 'T1', behaviour: 'core adds numbers', tests: 1 }]);
    await client.callTool({ name: 'guardrails_flag', arguments: { cwd: r.dir, specId: 'CORE-01', file: 'src/core/a.test.ts', reason: 'expects 3 for add(1,1); spec says sum' } });
    expect(JSON.parse(readFileSync(join(r.dir, '.guardrails/flags.json'), 'utf8')).flags).toMatchObject([{ id: 'F1', specId: 'CORE-01', by: 'mcp' }]);
    await client.close();
  });
});
