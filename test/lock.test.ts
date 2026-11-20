import { describe, expect, it } from 'vitest';
import { addFlag, checkFlags } from '../src/flags.js';
import { guard, hookResponse } from '../src/hook.js';
import { repo } from './helpers/repo.js';

const tagged = "it('CORE-01 adds', () => { expect(1).toBe(1) });\n";

describe('FLAG-01 agents stop and flag; open flags block until a human clears them', () => {
  it('blocks open flags, accepts the flags-resolved label, and warns when a PR clears a flag', () => {
    const r = repo().commit();
    expect(() => addFlag(r.dir, '.guardrails/flags.json', { specId: 'CORE-01', file: 'a', reason: 'no' })).toThrow('reason must explain');
    addFlag(r.dir, '.guardrails/flags.json', { specId: 'CORE-01', file: 'src/core/a.test.ts', reason: 'test expects 5 but spec says 4' }, new Date('2026-01-01'));
    expect(checkFlags(r.ctx()).violations.map((v) => v.message)).toEqual([
      'open flag F1 on CORE-01: test expects 5 but spec says 4. A human must resolve it before merge.',
    ]);
    expect(checkFlags(r.ctx(null, ['guardrails:flags-approved'])).violations).toEqual([]);
    r.commit();
    r.write({ '.guardrails/flags.json': '' });
    expect(checkFlags(r.ctx('HEAD')).violations.map((v) => `${v.maxMode} ${v.message.split(' in this')[0]}`)).toEqual(['warn flag F1 (CORE-01) was cleared']);
    expect(checkFlags(r.ctx('HEAD', ['guardrails:flags-approved'])).violations).toEqual([]);
  });
});

describe('LOCK-01 agent hooks deny edits to spec-tagged tests and guardrails config', () => {
  const project = () => repo({ 'src/core/a.test.ts': tagged, 'src/core/plain.test.ts': "it('x', () => {});\n" }).dir;
  const env = { CLAUDE_PROJECT_DIR: '$ROOT' };
  it.each([
    [{ tool_input: { file_path: '$ROOT/src/core/a.test.ts' } }, env, 'src/core/a.test.ts'],
    [{ tool_input: { file_path: 'guardrails.config.json' } }, env, 'guardrails.config.json'],
    [{ tool_input: { file_path: '.github/workflows/ci.yml' } }, env, '.github/workflows/ci.yml'],
    [{ toolCall: { name: 'write_to_file', args: { TargetFile: '$ROOT/.guardrails/baseline.json' } }, workspacePaths: ['$ROOT'] }, {}, '.guardrails/baseline.json'],
    [{ tool_input: { command: "sed -i 's/toBe(5)/toBe(4)/' src/core/a.test.ts" } }, env, 'src/core/a.test.ts'],
    [{ tool_input: { command: 'cat src/core/a.test.ts' } }, env, null],
    [{ tool_input: { command: 'npx guardrails flag CORE-01 "src/core/a.test.ts conflicts with spec"' } }, env, null],
    [{ tool_input: { file_path: 'src/core/new.test.ts' } }, env, null],
    [{ tool_input: { file_path: 'src/core/plain.test.ts' } }, env, null],
    [{ tool_input: { file_path: 'src/core/a.test.ts' } }, { ...env, GUARDRAILS_ALLOW_TEST_EDITS: '1' }, null],
  ])('%j', (payload, e, blocked) => {
    const root = project();
    const fill = <T>(x: T): T => JSON.parse(JSON.stringify(x).replaceAll('$ROOT', root));
    expect(guard(fill(payload), fill(e)).path ?? null).toBe(blocked);
  });

  it('speaks each agent hook protocol', () => {
    const deny = guard({ tool_input: { file_path: 'guardrails.config.json' } }, { CLAUDE_PROJECT_DIR: project() });
    expect([hookResponse('claude', deny).code, hookResponse('gemini', deny).code, JSON.parse(hookResponse('antigravity', deny).stdout).decision]).toEqual([2, 2, 'deny']);
    expect(hookResponse('claude', deny).stderr).toContain('call guardrails_flag');
    expect([hookResponse('claude', { allow: true }), hookResponse('antigravity', { allow: true }).stdout]).toEqual([{ code: 0, stdout: '', stderr: '' }, '{"decision":"allow"}']);
  });
});
