import { existsSync } from 'node:fs';
import { join, posix } from 'node:path';
import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { globToRegex, isTestFile, listFiles, readText, violation } from '../util.js';

const VITEST_CONFIGS = /^(vitest|vite)\.config\.(ts|mts|cts|js|mjs|cjs)$|^vitest\.workspace\.(ts|js|mjs)$/;
const SETUP_PACKAGE = '@ws42/guardrails/vitest-setup';

/** TEST-9: Vitest setupFiles must enforce expect.hasAssertions(). */
function checkAssertionSetup(ctx: CheckContext, files: string[]): Violation[] {
  if (!files.some((f) => isTestFile(ctx.config, f))) return [];
  const configs = files.filter((f) => VITEST_CONFIGS.test(f));
  const setupFiles: string[] = [];
  for (const c of configs) {
    const src = readText(ctx.cwd, c);
    for (const m of src.matchAll(/setupFiles\s*:\s*(\[[^\]]*\]|['"`][^'"`]+['"`])/g))
      for (const s of m[1].matchAll(/['"`]([^'"`]+)['"`]/g)) setupFiles.push(s[1]);
  }
  if (setupFiles.some((s) => s.includes(SETUP_PACKAGE))) return [];
  for (const s of setupFiles) {
    const candidates = [s, `${s}.ts`, `${s}.js`].map((p) => posix.normalize(p.replace(/^\.\//, '')));
    const hit = candidates.find((p) => existsSync(join(ctx.cwd, p)));
    if (hit && /expect\.hasAssertions\(\s*\)/.test(readText(ctx.cwd, hit))) return [];
  }
  const where = configs[0] ?? 'vitest.config.ts';
  return [violation('TEST-9', where, `no Vitest setup file calls expect.hasAssertions(). Add "${SETUP_PACKAGE}" to test.setupFiles.`, 'setup')];
}

interface OwnerRule { re: RegExp; owners: number }
function parseCodeowners(src: string): OwnerRule[] {
  const out: OwnerRule[] = [];
  for (const raw of src.split('\n')) {
    const line = raw.replace(/#.*/, '').trim();
    if (!line) continue;
    const [pattern, ...owners] = line.split(/\s+/);
    let p = pattern;
    const anchored = p.startsWith('/') || p.slice(0, -1).includes('/');
    p = p.replace(/^\//, '');
    if (p.endsWith('/')) p += '**';
    else if (!p.includes('*')) p = `${p}{,/**}`;
    out.push({ re: globToRegex(anchored ? p : `**/${p}`), owners: owners.length });
  }
  return out;
}

/** CFG-2: CODEOWNERS must give the config, the workflows and the baseline a human owner. */
function checkCodeowners(ctx: CheckContext, files: string[]): Violation[] {
  const file = ['.github/CODEOWNERS', 'CODEOWNERS', 'docs/CODEOWNERS'].find((f) => files.includes(f) || existsSync(join(ctx.cwd, f)));
  const required = ['guardrails.config.json', '.github/workflows/guardrails.yml', ctx.config.baselineFile];
  if (!file) return [violation('CFG-2', '.github/CODEOWNERS', `missing. It must assign an owner to ${required.join(', ')}.`, 'missing')];
  const rules = parseCodeowners(readText(ctx.cwd, file));
  const v: Violation[] = [];
  for (const path of required) {
    const last = [...rules].reverse().find((r) => r.re.test(path));
    if (!last || last.owners === 0) v.push(violation('CFG-2', file, `does not assign an owner to ${path}.`, `path:${path}`));
  }
  return v;
}

export function checkSetup(ctx: CheckContext, files = listFiles(ctx.cwd)): CheckOutput {
  return { violations: [...checkAssertionSetup(ctx, files), ...checkCodeowners(ctx, files)] };
}
