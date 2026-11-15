import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const TEMPLATE = join(dirname(fileURLToPath(import.meta.url)), '..', 'template');
const FILES = ['guardrails.config.json', 'AGENTS.md', 'GEMINI.md', 'CLAUDE.md', '.github/CODEOWNERS', '.github/workflows/guardrails.yml'];

export interface InitOptions { cwd: string; owner?: string; base?: string; force?: boolean }
export interface InitResult { written: string[]; skipped: string[] }

/** Guess tiers, roots and entries from what exists, so a fresh repo starts close to right. */
export function guessConfig(cwd: string, template: Record<string, unknown>, base: string): Record<string, unknown> {
  const has = (p: string) => existsSync(join(cwd, p));
  const roots = ['src', 'functions', 'app', 'server', 'lib', 'api'].filter(has);
  const T1 = ['functions/api/**', 'src/lib/storage/**', 'src/lib/auth/**', 'src/lib/auth.ts', 'src/server/**', 'api/**', 'server/**'].filter((g) => has(g.replace(/\/\*\*$/, '')));
  const entries = ['src/main.tsx', 'src/main.ts', 'src/index.ts', 'src/index.tsx', 'src/pages/**/*.astro', 'functions/**/*.ts'].filter((g) => has(g.split('/').filter((_, i, a) => !a.slice(0, i + 1).some((x) => x.includes('*'))).join('/')));
  const cfg = { ...template, baseBranch: base, sourceRoots: roots.length ? roots : ['src'] } as Record<string, any>;
  cfg.tiers = { T1, T2: has('src/lib') ? ['src/lib/**'] : [], T3: roots.map((r) => `${r}/**`) };
  cfg.architecture = { ...cfg.architecture, entries: entries.length ? entries : cfg.architecture.entries };
  return cfg;
}

export function init(opts: InitOptions): InitResult {
  const owner = opts.owner ?? '@OWNER';
  const base = opts.base ?? 'main';
  const written: string[] = [];
  const skipped: string[] = [];
  for (const f of FILES) {
    const dest = join(opts.cwd, f);
    if (existsSync(dest) && !opts.force) {
      skipped.push(f);
      continue;
    }
    let body = readFileSync(join(TEMPLATE, f), 'utf8').replaceAll('{{OWNER}}', owner).replaceAll('{{BASE}}', base);
    if (f === 'guardrails.config.json') body = `${JSON.stringify(guessConfig(opts.cwd, JSON.parse(body), base), null, 2)}\n`;
    mkdirSync(dirname(dest), { recursive: true });
    writeFileSync(dest, body);
    written.push(f);
  }
  return { written, skipped };
}
