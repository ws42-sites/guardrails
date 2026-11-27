import { describe, expect, it } from 'vitest';
import { guard } from '../src/hook.js';
import { repo } from './helpers/repo.js';

// Gitleaks reads these from the repo, so an agent that could edit them could exempt its own secrets from SEC-1.
describe('LOCK-01 agent hooks lock the gitleaks settings by default', () => {
  it.each([
    [{ tool_input: { file_path: '.gitleaks.toml' } }, '.gitleaks.toml'],
    [{ tool_input: { file_path: '$ROOT/.gitleaksignore' } }, '.gitleaksignore'],
    [{ toolCall: { name: 'write_to_file', args: { TargetFile: '$ROOT/.gitleaks.toml' } }, workspacePaths: ['$ROOT'] }, '.gitleaks.toml'],
    [{ tool_input: { command: 'echo "abc123:src/a.ts:generic-api-key:4" >> .gitleaksignore' } }, '.gitleaksignore'],
    [{ tool_name: 'apply_patch', tool_input: { command: '*** Begin Patch\n*** Add File: .gitleaks.toml\n+[extend]\n*** End Patch' } }, '.gitleaks.toml'],
    [{ tool_input: { command: 'cat .gitleaks.toml' } }, null],
    [{ tool_input: { file_path: 'src/gitleaks.ts' } }, null],
  ])('%j', (payload, blocked) => {
    const root = repo().dir;
    const fill = <T>(x: T): T => JSON.parse(JSON.stringify(x).replaceAll('$ROOT', JSON.stringify(root).slice(1, -1)));
    expect(guard(fill(payload), { CLAUDE_PROJECT_DIR: root }).path ?? null).toBe(blocked);
  });
});
