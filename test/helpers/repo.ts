import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach } from 'vitest';
import { parseConfig, type GuardrailsConfig } from '../../src/config.js';
import type { CheckContext } from '../../src/types.js';

const made: string[] = [];
afterEach(() => {
  while (made.length) rmSync(made.pop() as string, { recursive: true, force: true });
});

export interface Repo {
  dir: string;
  write(files: Record<string, string>): Repo;
  remove(...files: string[]): Repo;
  commit(msg?: string): Repo;
  ctx(base?: string | null, labels?: string[]): CheckContext;
}

export const BASE_CONFIG = {
  version: 1,
  sourceRoots: ['src'],
  tiers: { T1: ['src/core/**'], T2: ['src/lib/**'], T3: [] },
  specs: { items: { 'CORE-01': { tier: 'T1', behaviour: 'core adds numbers' }, 'LIB-01': { tier: 'T2', behaviour: 'lib formats text' } } },
} as const;

/** A throwaway git repo with a guardrails config. Removed after each test. */
export function repo(files: Record<string, string> = {}, config: object = BASE_CONFIG): Repo {
  const dir = mkdtempSync(join(tmpdir(), 'guardrails-test-'));
  made.push(dir);
  const git = (...args: string[]) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false', ...args], { cwd: dir, stdio: 'pipe' });
  git('init', '-q', '-b', 'main');
  const r: Repo = {
    dir,
    write(more) {
      for (const [f, body] of Object.entries(more)) {
        mkdirSync(dirname(join(dir, f)), { recursive: true });
        writeFileSync(join(dir, f), body);
      }
      return r;
    },
    remove(...fs) {
      for (const f of fs) rmSync(join(dir, f));
      return r;
    },
    commit(msg = 'c') {
      git('add', '-A');
      git('commit', '-q', '--allow-empty', '-m', msg);
      return r;
    },
    ctx(base = null, labels = []) {
      const cfg: GuardrailsConfig = parseConfig(JSON.parse(readFileSync(join(dir, 'guardrails.config.json'), 'utf8')));
      return { cwd: dir, config: cfg, base, labels, options: { clockShift: false, mutation: false, knip: false } };
    },
  };
  r.write({ 'guardrails.config.json': JSON.stringify(config), 'AGENTS.md': '# Agents\n', ...files });
  return r;
}

export const rules = (vs: { rule: string }[]) => vs.map((v) => v.rule).sort();
