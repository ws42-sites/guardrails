import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from './config.js';
import { auditSuite, readSuiteBaseline, toBaseline, writeSuiteBaseline } from './test-suite.js';
import { listFiles } from './util.js';

const TEMPLATE = join(dirname(fileURLToPath(import.meta.url)), '..', 'template');
const FILES = ['guardrails.config.json', 'AGENTS.md', 'GEMINI.md', 'CLAUDE.md', '.github/CODEOWNERS', '.github/workflows/guardrails.yml', ...HOOK_FILES(), '.codex/config.toml'];
export function initTemplateFiles(): string[] {
  return [...FILES, '.gemini/.env', 'RUNBOOK.md', 'TESTING.md'];
}
/** Agent settings: hooks that lock spec-tagged tests and guardrails config, plus the MCP server. Merged into existing files. */
function HOOK_FILES(): string[] {
  return ['.claude/settings.json', '.mcp.json', '.gemini/settings.json', '.agents/hooks.json', '.agents/mcp_config.json', '.codex/hooks.json'];
}
type Json = Record<string, unknown>;
/** Append the template's TOML tables that an existing Codex config lacks. Never edits or duplicates a table; null when nothing is missing. */
export function mergeCodexToml(existing: string, template: string): string | null {
  const has = (re: RegExp) => re.test(existing);
  const add = template.split(/\n\n+/).map((b) => b.trim()).filter((block) => {
    const table = /^\[([^\]]+)\]/m.exec(block)?.[1];
    if (!table) return false;
    if (table.startsWith('shell_environment_policy')) return !has(/^\s*(\[shell_environment_policy|shell_environment_policy\b)/m);
    return !has(new RegExp(`^\\s*\\[${table.replace(/\./g, '\\.')}\\]`, 'm'));
  });
  return add.length ? `${existing.replace(/\s*$/, '')}\n\n${add.join('\n\n')}\n` : null;
}
/** Deep-merge template JSON into existing settings: objects merge, arrays append entries not already present. */
export function mergeSettings(existing: Json, add: Json): Json {
  const out: Json = { ...existing };
  for (const [k, v] of Object.entries(add)) {
    const cur = out[k];
    if (Array.isArray(cur) && Array.isArray(v)) out[k] = [...cur, ...v.filter((x) => !cur.some((c) => JSON.stringify(c) === JSON.stringify(x)))];
    else if (cur && v && typeof cur === 'object' && typeof v === 'object' && !Array.isArray(cur)) out[k] = mergeSettings(cur as Json, v as Json);
    else if (cur === undefined) out[k] = v;
  }
  return out;
}

export interface InitOptions { cwd: string; owner?: string; base?: string; force?: boolean; preset?: string }
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
  const site = opts.preset === 'site';
  const files = site ? ['guardrails.config.json', '.github/workflows/guardrails.yml'] : FILES;
  for (const f of files) {
    const dest = join(opts.cwd, f);
    const source = site ? `site/${f === '.github/workflows/guardrails.yml' ? 'workflow.yml' : f}` : f;
    let body = readFileSync(join(TEMPLATE, source), 'utf8').replaceAll('{{OWNER}}', owner).replaceAll('{{BASE}}', base);
    if (existsSync(dest) && !opts.force) {
      if (f === '.codex/config.toml') {
        const merged = mergeCodexToml(readFileSync(dest, 'utf8'), body);
        if (merged === null) { skipped.push(f); continue; }
        body = merged;
      } else if (!HOOK_FILES().includes(f)) {
        skipped.push(f);
        continue;
      } else try {
        const current = JSON.parse(readFileSync(dest, 'utf8'));
        const merged = `${JSON.stringify(mergeSettings(current, JSON.parse(body)), null, 2)}\n`;
        if (merged === `${JSON.stringify(current, null, 2)}\n`) { skipped.push(f); continue; }
        body = merged;
      } catch {
        skipped.push(f);
        continue;
      }
    }
    if (!site && f === 'guardrails.config.json') body = `${JSON.stringify(guessConfig(opts.cwd, JSON.parse(body), base), null, 2)}\n`;
    mkdirSync(dirname(dest), { recursive: true });
    writeFileSync(dest, body);
    written.push(f);
  }
  if (site) return { written, skipped };
  // Gemini CLI loads .gemini/.env: mark its shell as an agent so human-only commands refuse.
  const envFile = join(opts.cwd, '.gemini/.env');
  const line = readFileSync(join(TEMPLATE, '.gemini/.env'), 'utf8');
  const cur = existsSync(envFile) ? readFileSync(envFile, 'utf8') : null;
  if (cur === null || !/^GUARDRAILS_AGENT=/m.test(cur)) {
    mkdirSync(dirname(envFile), { recursive: true });
    writeFileSync(envFile, cur === null ? line : `${cur.replace(/\n?$/, '\n')}${line}`);
    written.push('.gemini/.env');
  } else skipped.push('.gemini/.env');
  return { written, skipped };
}

const STUB = (name: string) => `echo "${name}: not wired yet. Point it at the real command or mark it n/a in guardrails.config.json runbook.na." && exit 1`;

/** guardrails init runbook: RUNBOOK.md template, stub standard scripts, TESTING.md section, suite baseline; enables runbook, ci, testSuite. Never overwrites. */
export function initRunbook(opts: { cwd: string; force?: boolean }): InitResult {
  const written: string[] = [];
  const skipped: string[] = [];
  const dest = join(opts.cwd, 'RUNBOOK.md');
  if (existsSync(dest) && !opts.force) skipped.push('RUNBOOK.md');
  else { writeFileSync(dest, readFileSync(join(TEMPLATE, 'RUNBOOK.md'), 'utf8')); written.push('RUNBOOK.md'); }
  const edit = (file: string, fn: (j: Json) => boolean) => {
    const p = join(opts.cwd, file);
    if (!existsSync(p)) { skipped.push(file); return; }
    const j = JSON.parse(readFileSync(p, 'utf8')) as Json;
    if (fn(j)) { writeFileSync(p, `${JSON.stringify(j, null, 2)}\n`); written.push(file); } else skipped.push(file);
  };
  edit('package.json', (pkg) => {
    const scripts = (pkg.scripts ??= {}) as Record<string, string>;
    const missing = ['deploy', 'deploy:preview', 'rollback', 'smoke', 'secrets:check', 'test:changed', 'test:full'].filter((s) => !(s in scripts));
    for (const s of missing) scripts[s] = STUB(s);
    if (!('check:tests' in scripts)) { scripts['check:tests'] = 'guardrails check-tests'; missing.push('check:tests'); }
    return missing.length > 0;
  });
  edit('guardrails.config.json', (cfg) => {
    const rb = (cfg.runbook ??= {}) as Json;
    const ci = (cfg.ci ??= {}) as Json;
    const ts = (cfg.testSuite ??= {}) as Json;
    if (rb.enabled === true && ci.enabled === true && ts.enabled === true) return false;
    rb.enabled = true;
    ci.enabled = true;
    ts.enabled = true;
    return true;
  });
  // TESTING.md gets the short test-suite section; existing findings go into the suite baseline.
  const testing = join(opts.cwd, 'TESTING.md');
  const section = readFileSync(join(TEMPLATE, 'TESTING.md'), 'utf8');
  if (!existsSync(testing)) { writeFileSync(testing, section); written.push('TESTING.md'); }
  else if (!readFileSync(testing, 'utf8').includes('## Test suite standard')) {
    writeFileSync(testing, `${readFileSync(testing, 'utf8').trimEnd()}\n\n${section.slice(section.indexOf('## Test suite standard'))}`);
    written.push('TESTING.md');
  } else skipped.push('TESTING.md');
  if (existsSync(join(opts.cwd, 'guardrails.config.json'))) {
    const cfg = loadConfig(opts.cwd);
    if (!readSuiteBaseline(opts.cwd, cfg.testSuite.baselineFile)) {
      writeSuiteBaseline(opts.cwd, cfg.testSuite.baselineFile, toBaseline(auditSuite(opts.cwd, cfg, listFiles(opts.cwd)), cfg.testSuite.maxLines));
      written.push(cfg.testSuite.baselineFile);
    }
  }
  return { written, skipped };
}
