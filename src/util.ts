import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import type { Tier, Violation } from './types.js';
import type { GuardrailsConfig } from './config.js';

const globCache = new Map<string, RegExp>();
/** Minimal glob: ** any depth, * one segment, ? one char, {a,b} alternatives, trailing / = folder. */
export function globToRegex(glob: string): RegExp {
  const hit = globCache.get(glob);
  if (hit) return hit;
  const g = glob.endsWith('/') ? `${glob}**` : glob;
  let re = '';
  for (let i = 0; i < g.length; i++) {
    const c = g[i];
    if (c === '*' && g[i + 1] === '*') {
      const slash = g[i + 2] === '/';
      re += slash ? '(?:.*/|)' : '.*';
      i += slash ? 2 : 1;
    } else if (c === '*') re += '[^/]*';
    else if (c === '?') re += '[^/]';
    else if (c === '{') re += '(?:';
    else if (c === '}') re += ')';
    else if (c === ',') re += '|';
    else re += c.replace(/[.+^$()|[\]\\]/g, '\\$&');
  }
  const out = new RegExp(`^${re}$`);
  globCache.set(glob, out);
  return out;
}
export const matchesAny = (file: string, globs: readonly string[]): boolean =>
  globs.some((g) => globToRegex(g).test(file));

export const SKIP_DIRS = new Set(['.git', 'node_modules', 'dist', 'build', 'coverage', 'reports', '.wrangler', '.stryker-tmp', '.astro', '.next', '.turbo']);

export function walk(cwd: string, dir = ''): string[] {
  const out: string[] = [];
  const abs = join(cwd, dir);
  if (!existsSync(abs) || !statSync(abs).isDirectory()) return out;
  for (const e of readdirSync(abs, { withFileTypes: true })) {
    if (SKIP_DIRS.has(e.name)) continue;
    const rel = dir ? `${dir}/${e.name}` : e.name;
    if (e.isDirectory()) out.push(...walk(cwd, rel));
    else if (e.isFile()) out.push(rel);
  }
  return out;
}

/** Files git tracks (plus untracked, not ignored). Falls back to a walk outside git. */
export function listFiles(cwd: string): string[] {
  try {
    const out = execFileSync('git', ['ls-files', '-co', '--exclude-standard'], { cwd, encoding: 'utf8', maxBuffer: 64 << 20 });
    return out.split('\n').filter((f) => f && existsSync(join(cwd, f)));
  } catch {
    return walk(cwd);
  }
}

export const readText = (cwd: string, f: string): string => readFileSync(join(cwd, f), 'utf8');
export const lineCount = (text: string): number => (text === '' ? 0 : text.replace(/\n$/, '').split('\n').length);
export const fingerprint = (...parts: string[]): string => createHash('sha1').update(parts.join('\u0000')).digest('hex').slice(0, 12);

export function violation(rule: string, file: string, message: string, key: string, line?: number): Violation {
  return { rule, file, line, message, fingerprint: fingerprint(rule, file, key) };
}

export const SOURCE_EXT = /\.(ts|tsx|js|jsx|mjs|cjs|mts|cts)$/;
export const isTestFile = (cfg: GuardrailsConfig, f: string): boolean => matchesAny(f, cfg.tests.globs);
export const isHelperFile = (cfg: GuardrailsConfig, f: string): boolean =>
  !isTestFile(cfg, f) && (matchesAny(f, cfg.tests.helperGlobs) || matchesAny(f, cfg.fixtures.globs));

export function isProductFile(cfg: GuardrailsConfig, f: string): boolean {
  return SOURCE_EXT.test(f) && matchesAny(f, cfg.sourceRoots.map((r) => (/[*]/.test(r) ? r : `${r.replace(/\/$/, '')}/**`))) && !isTestFile(cfg, f) && !isHelperFile(cfg, f);
}

export function tierOf(cfg: GuardrailsConfig, f: string): Tier {
  if (matchesAny(f, cfg.tiers.T1)) return 'T1';
  if (matchesAny(f, cfg.tiers.T2)) return 'T2';
  return 'T3';
}

// ---- git ----
export function git(cwd: string, args: string[]): string {
  return execFileSync('git', args, { cwd, encoding: 'utf8', maxBuffer: 128 << 20, stdio: ['ignore', 'pipe', 'pipe'] });
}
export function mergeBase(cwd: string, base: string): string {
  try {
    return git(cwd, ['merge-base', base, 'HEAD']).trim();
  } catch {
    return base;
  }
}
export function showAt(cwd: string, ref: string, file: string): string | null {
  try {
    return git(cwd, ['show', `${ref}:${file}`]);
  } catch {
    return null;
  }
}
export function filesAt(cwd: string, ref: string): string[] {
  try {
    return git(cwd, ['ls-tree', '-r', '--name-only', ref]).split('\n').filter(Boolean);
  } catch {
    return [];
  }
}

export interface FileDiff {
  added: string[];
  removed: string[];
  status: 'added' | 'deleted' | 'modified';
}
/** Working tree (including uncommitted changes) vs merge-base(base, HEAD). */
export function diffAgainst(cwd: string, base: string): Record<string, FileDiff> {
  const mb = mergeBase(cwd, base);
  const out = git(cwd, ['diff', '-U0', '--no-color', '--no-renames', mb]);
  const files: Record<string, FileDiff> = {};
  let cur: FileDiff | null = null;
  let oldPath: string | null = null;
  let header = false;
  for (const line of out.split('\n')) {
    if (line.startsWith('diff --git ')) {
      header = true;
      cur = null;
      oldPath = null;
      continue;
    }
    if (header && line.startsWith('--- ')) {
      const p = line.slice(4);
      oldPath = p === '/dev/null' ? null : p.replace(/^a\//, '');
      continue;
    }
    if (header && line.startsWith('+++ ')) {
      const p = line.slice(4);
      const path = p === '/dev/null' ? oldPath : p.replace(/^b\//, '');
      if (path) {
        cur = { added: [], removed: [], status: p === '/dev/null' ? 'deleted' : oldPath === null ? 'added' : 'modified' };
        files[path] = cur;
      }
      continue;
    }
    if (line.startsWith('@@')) {
      header = false;
      continue;
    }
    if (header || !cur) continue;
    if (line.startsWith('+')) cur.added.push(line.slice(1));
    else if (line.startsWith('-')) cur.removed.push(line.slice(1));
  }
  // Untracked files are not in `git diff`; count them as added.
  try {
    for (const f of git(cwd, ['ls-files', '--others', '--exclude-standard']).split('\n').filter(Boolean)) {
      if (!files[f] && existsSync(join(cwd, f))) files[f] = { added: readText(cwd, f).split('\n'), removed: [], status: 'added' };
    }
  } catch {
    /* not a git repo */
  }
  return files;
}
