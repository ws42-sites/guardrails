import { existsSync, lstatSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from './config.js';
import { auditSuite, readSuiteBaseline, toBaseline, writeSuiteBaseline } from './test-suite.js';
import { listFiles, SKIP_DIRS } from './util.js';

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
/** `written` is every file init changed; `merged` is the part of it that was added into a file that already existed. */
export interface InitResult { written: string[]; skipped: string[]; merged?: string[] }

export const MARK_START = '<!-- guardrails:start -->';
export const MARK_END = '<!-- guardrails:end -->';
const POINTER = `${MARK_START} See AGENTS.md for the guardrails rules. ${MARK_END}`;
const eolOf = (text: string) => (text.includes('\r\n') ? '\r\n' : '\n');

/** Insert `block` (markers included) at the end of `existing`, or replace only what sits between its markers. null when nothing would change or the markers are broken. */
export function upsertMarked(existing: string, block: string): string | null {
  const eol = eolOf(existing);
  const start = existing.indexOf(MARK_START);
  const end = existing.indexOf(MARK_END, start + 1);
  if (start === -1 && existing.includes(MARK_END)) return null;
  if (start !== -1) {
    if (end === -1) return null;
    const next = existing.slice(0, start) + block.replace(/\r?\n/g, eol) + existing.slice(end + MARK_END.length);
    return next === existing ? null : next;
  }
  return `${existing.replace(/\s*$/, '')}${eol}${eol}${block.replace(/\r?\n/g, eol)}${eol}`;
}

/** Template owner lines (comment lines excluded) whose path the existing CODEOWNERS never mentions, appended after it so they win. null when none are missing. */
export function mergeCodeowners(existing: string, template: string): string | null {
  const paths = new Set(existing.split(/\r?\n/).map((l) => l.trim()).filter((l) => l && !l.startsWith('#')).map((l) => l.split(/\s+/)[0]));
  const lines = template.split(/\r?\n/).filter((l) => l.trim());
  const missing = lines.filter((l) => !l.startsWith('#') && !paths.has(l.trim().split(/\s+/)[0]));
  if (!missing.length) return null;
  const eol = eolOf(existing);
  const head = lines.filter((l) => l.startsWith('#'));
  return `${existing.replace(/\s*$/, '')}${eol}${eol}${[...head, ...missing].join(eol)}${eol}`;
}

/** The existing agent file (AGENTS.md, CLAUDE.md, GEMINI.md) with guardrails added; null to skip it. */
function mergeAgentFile(file: string, current: string, body: string, dest: string): string | null {
  if (file === 'AGENTS.md') {
    const from = body.indexOf(MARK_START);
    const to = body.indexOf(MARK_END);
    return upsertMarked(current, body.slice(from, to + MARK_END.length));
  }
  if (lstatSync(dest).isSymbolicLink()) return null;
  if (current.includes(MARK_START) || current.includes(MARK_END)) return upsertMarked(current, POINTER);
  return current.includes('AGENTS.md') ? null : upsertMarked(current, POINTER);
}

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
  const merged: string[] = [];
  const site = opts.preset === 'site';
  const files = site ? ['guardrails.config.json', '.github/workflows/guardrails.yml'] : FILES;
  for (const f of files) {
    const dest = join(opts.cwd, f);
    const source = site ? `site/${f === '.github/workflows/guardrails.yml' ? 'workflow.yml' : f}` : f;
    let body = readFileSync(join(TEMPLATE, source), 'utf8').replaceAll('{{OWNER}}', owner).replaceAll('{{BASE}}', base);
    if (existsSync(dest) && !opts.force) {
      if (f === '.codex/config.toml') {
        const next = mergeCodexToml(readFileSync(dest, 'utf8'), body);
        if (next === null) { skipped.push(f); continue; }
        body = next;
      } else if (['AGENTS.md', 'CLAUDE.md', 'GEMINI.md'].includes(f)) {
        const next = mergeAgentFile(f, readFileSync(dest, 'utf8'), body, dest);
        if (next === null) { skipped.push(f); continue; }
        body = next;
      } else if (f.endsWith('CODEOWNERS')) {
        const next = mergeCodeowners(readFileSync(dest, 'utf8'), body);
        if (next === null) { skipped.push(f); continue; }
        body = next;
      } else if (!HOOK_FILES().includes(f)) {
        skipped.push(f);
        continue;
      } else try {
        const current = JSON.parse(readFileSync(dest, 'utf8'));
        const next = `${JSON.stringify(mergeSettings(current, JSON.parse(body)), null, 2)}\n`;
        if (next === `${JSON.stringify(current, null, 2)}\n`) { skipped.push(f); continue; }
        body = next;
      } catch {
        skipped.push(f);
        continue;
      }
    }
    if (!site && f === 'guardrails.config.json') body = `${JSON.stringify(guessConfig(opts.cwd, JSON.parse(body), base), null, 2)}\n`;
    if (existsSync(dest) && !opts.force) merged.push(f);
    mkdirSync(dirname(dest), { recursive: true });
    writeFileSync(dest, body);
    written.push(f);
  }
  if (!site) {
    const envFile = join(opts.cwd, '.gemini/.env');
    const line = readFileSync(join(TEMPLATE, '.gemini/.env'), 'utf8');
    const cur = existsSync(envFile) ? readFileSync(envFile, 'utf8') : null;
    if (cur === null || !/^GUARDRAILS_AGENT=/m.test(cur)) {
      mkdirSync(dirname(envFile), { recursive: true });
      writeFileSync(envFile, cur === null ? line : `${cur.replace(/\n?$/, '\n')}${line}`);
      if (cur !== null) merged.push('.gemini/.env');
      written.push('.gemini/.env');
    } else skipped.push('.gemini/.env');
  }
  const ignore = ensureGitignore(opts.cwd);
  if (ignore === 'skipped') skipped.push('.gitignore');
  else {
    written.push('.gitignore');
    if (ignore === 'merged') merged.push('.gitignore');
  }
  if (site) return merged.length ? { written, skipped, merged } : { written, skipped };
  return { written, skipped, merged };
}

const GITIGNORE_NAMES = [...SKIP_DIRS].filter((name) => name !== '.git');

function ignoreKey(line: string): string {
  const text = line.trim();
  if (!text || text.startsWith('#') || text.startsWith('!')) return '';
  return text.replace(/^\//, '').replace(/\/\*\*$/, '').replace(/\/$/, '');
}

function ensureGitignore(cwd: string): 'created' | 'merged' | 'skipped' {
  const dest = join(cwd, '.gitignore');
  const entries = GITIGNORE_NAMES.map((name) => `${name}/`);
  if (!existsSync(dest)) {
    writeFileSync(dest, `${entries.join('\n')}\n`);
    return 'created';
  }
  const current = readFileSync(dest, 'utf8');
  const have = new Set(current.split(/\r?\n/).map(ignoreKey));
  const missing = entries.filter((entry) => !have.has(ignoreKey(entry)));
  if (!missing.length) return 'skipped';
  const eol = current.includes('\r\n') ? '\r\n' : '\n';
  writeFileSync(dest, `${current.replace(/\s*$/, '')}${eol}${missing.join(eol)}${eol}`);
  return 'merged';
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
