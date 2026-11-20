import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { init } from '../src/init.js';

const dirs: string[] = [];
afterEach(() => {
  while (dirs.length) rmSync(dirs.pop() as string, { recursive: true, force: true });
});
const project = (files: Record<string, string>) => {
  const dir = mkdtempSync(join(tmpdir(), 'guardrails-init-'));
  dirs.push(dir);
  for (const [f, body] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, f)), { recursive: true });
    writeFileSync(join(dir, f), body);
  }
  return dir;
};
const read = (dir: string, f: string) => readFileSync(join(dir, f), 'utf8');
const count = (text: string, part: string) => text.split(part).length - 1;
const START = '<!-- guardrails:start -->';
const END = '<!-- guardrails:end -->';

describe('INIT-01 init pulls guardrails into agent files that already exist', () => {
  it('appends the rules to an existing AGENTS.md between markers and leaves the rest alone', () => {
    const mine = '# My project\n\nUse tabs.\n';
    const dir = project({ 'AGENTS.md': mine });
    const res = init({ cwd: dir });
    const agents = read(dir, 'AGENTS.md');
    expect([agents.startsWith(`${mine}\n${START}\n## Guardrails\n`), agents.endsWith(`${END}\n`), count(agents, START), count(agents, END)]).toEqual([true, true, 1, 1]);
    expect(agents).toContain('npx guardrails check --base origin/main');
    expect(agents).not.toContain('{{');
    expect([res.written, res.merged, res.skipped].map((l) => l?.includes('AGENTS.md'))).toEqual([true, true, false]);
  });

  it('appends a marked one-line pointer to an existing CLAUDE.md, and skips a GEMINI.md that already mentions AGENTS.md or a symlinked CLAUDE.md', () => {
    const dir = project({ 'CLAUDE.md': '# Claude notes\nBe terse.\n', 'GEMINI.md': 'Read AGENTS.md first.\n' });
    const res = init({ cwd: dir });
    const claude = read(dir, 'CLAUDE.md');
    expect(claude).toBe(`# Claude notes\nBe terse.\n\n${START} See AGENTS.md for the guardrails rules. ${END}\n`);
    expect(read(dir, 'GEMINI.md')).toBe('Read AGENTS.md first.\n');
    expect([res.merged?.includes('CLAUDE.md'), res.skipped.includes('GEMINI.md'), res.written.includes('GEMINI.md')]).toEqual([true, true, false]);
    const linked = project({ 'AGENTS.md': '# Agents\n' });
    symlinkSync('AGENTS.md', join(linked, 'CLAUDE.md'));
    expect([init({ cwd: linked }).skipped.includes('CLAUDE.md'), count(read(linked, 'AGENTS.md'), START)]).toEqual([true, 1]);
  });

  it('is idempotent: a second run changes nothing and a stale section is replaced in place', () => {
    const dir = project({ 'AGENTS.md': '# Top\n', 'CLAUDE.md': 'notes\n' });
    init({ cwd: dir });
    const once = [read(dir, 'AGENTS.md'), read(dir, 'CLAUDE.md')];
    const again = init({ cwd: dir });
    expect([read(dir, 'AGENTS.md'), read(dir, 'CLAUDE.md')]).toEqual(once);
    expect([again.written, again.merged, again.skipped.includes('AGENTS.md'), again.skipped.includes('CLAUDE.md')]).toEqual([[], [], true, true]);

    const stale = once[0].replace(/## Guardrails[\s\S]*?(?=<!-- guardrails:end -->)/, 'old rules\n');
    writeFileSync(join(dir, 'AGENTS.md'), `${stale.replace('# Top', '# Top (edited)')}\nTrailing notes\n`);
    init({ cwd: dir });
    const fixed = read(dir, 'AGENTS.md');
    expect([fixed.startsWith('# Top (edited)\n'), fixed.endsWith(`${END}\n\nTrailing notes\n`), fixed.includes('old rules'), count(fixed, START)]).toEqual([true, true, false, 1]);
  });

  it('preserves CRLF line endings and skips a file whose markers are broken', () => {
    const dir = project({ 'AGENTS.md': '# Win\r\n', 'CLAUDE.md': `${START} dangling\n` });
    const res = init({ cwd: dir });
    const agents = read(dir, 'AGENTS.md');
    expect([agents.replace(/\r\n/g, '').includes('\n'), agents.startsWith('# Win\r\n\r\n')]).toEqual([false, true]);
    expect([res.skipped.includes('CLAUDE.md'), read(dir, 'CLAUDE.md')]).toEqual([true, `${START} dangling\n`]);
  });

  it('appends only the missing owner lines to an existing CODEOWNERS', () => {
    const mine = '* @team\n/guardrails.config.json @cfg-owner\n';
    const dir = project({ '.github/CODEOWNERS': mine });
    const res = init({ cwd: dir, owner: '@josh' });
    const owners = read(dir, '.github/CODEOWNERS');
    expect(owners.startsWith(mine)).toBe(true);
    expect([count(owners, '/guardrails.config.json'), owners.includes('/.github/workflows/ ') && owners.includes('@josh'), owners.includes('/.guardrails/ ')]).toEqual([1, true, true]);
    expect(res.merged).toContain('.github/CODEOWNERS');
    const again = init({ cwd: dir, owner: '@josh' });
    expect([read(dir, '.github/CODEOWNERS'), again.skipped.includes('.github/CODEOWNERS')]).toEqual([owners, true]);
  });

  it('keeps --force overwriting the whole file', () => {
    const dir = project({ 'AGENTS.md': '# Mine\n', 'CLAUDE.md': 'mine\n', '.github/CODEOWNERS': '* @team\n' });
    const res = init({ cwd: dir, force: true });
    expect([read(dir, 'AGENTS.md').includes('# Mine'), read(dir, 'CLAUDE.md'), read(dir, '.github/CODEOWNERS').includes('* @team'), res.merged?.includes('AGENTS.md')]).toEqual([false, 'See AGENTS.md.\n', false, false]);
  });

  it('prints created, merged and skipped from the CLI', () => {
    const dir = project({ 'AGENTS.md': '# Mine\n', 'CLAUDE.md': 'See AGENTS.md\n', '.github/CODEOWNERS': '* @team\n' });
    const out = spawnSync(process.execPath, [resolve('dist/cli.js'), 'init', '--owner', '@josh'], { cwd: dir, encoding: 'utf8' }).stdout;
    expect(out).toContain('merged  AGENTS.md');
    expect(out).toContain('merged  .github/CODEOWNERS');
    expect(out).toContain('created GEMINI.md');
    expect(out).toMatch(/skipped CLAUDE\.md/);
  });
});
