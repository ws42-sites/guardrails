import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import type { CheckContext, CheckOutput, Metric, Violation } from '../types.js';
import {
  globToRegex, isHelperFile, isProductFile, isTestFile, listFiles, matchesAny, mergeBase, readText, showAt, SOURCE_EXT, violation,
} from '../util.js';

const require = createRequire(import.meta.url);
const rx = (globs: string[]): string => globs.map((g) => globToRegex(g).source).join('|');

interface DcViolation { from: string; to: string; rule: { name: string; severity: string } }

/** ARCH-1 boundaries and ARCH-6 fixture imports, via dependency-cruiser. */
async function cruiseBoundaries(ctx: CheckContext, files: string[]): Promise<Violation[]> {
  const cfg = ctx.config;
  const notProd = [...cfg.tests.globs, ...cfg.tests.helperGlobs, ...cfg.fixtures.globs, ...cfg.fixtures.devOnly];
  const forbidden: object[] = cfg.architecture.forbidden.map((r, i) => ({
    name: `ARCH-1:${r.name ?? i}`,
    comment: r.message ?? '',
    severity: 'error',
    from: { path: rx(r.from), pathNot: rx([...notProd, ...(r.except ?? [])]) },
    to: { path: rx(r.to) },
  }));
  forbidden.push({
    name: 'ARCH-6:fixture-import',
    comment: 'Production code imports a fixture. Only tests and dev-only code may.',
    severity: 'error',
    from: { path: rx(cfg.sourceRoots.map((r) => (r.includes('*') ? r : `${r.replace(/\/$/, '')}/**`))), pathNot: rx(notProd) },
    to: { path: rx(cfg.fixtures.globs) },
  });
  const roots = files.filter((f) => isProductFile(cfg, f) || isHelperFile(cfg, f));
  if (!roots.length) return [];

  const { cruise } = await import('dependency-cruiser');
  const extractTSConfig = (await import('dependency-cruiser/config-utl/extract-ts-config')).default as (p: string) => unknown;
  const tsConfigFile = cfg.architecture.tsConfig ?? (existsSync(join(ctx.cwd, 'tsconfig.json')) ? 'tsconfig.json' : undefined);
  {
    const result = await cruise(
      roots,
      {
        validate: true,
        baseDir: ctx.cwd,
        ruleSet: { forbidden } as never,
        doNotFollow: { path: 'node_modules' },
        tsPreCompilationDeps: true,
        ...(tsConfigFile ? { tsConfig: { fileName: join(ctx.cwd, tsConfigFile) } } : {}),
      } as never,
      (tsConfigFile ? { tsConfig: join(ctx.cwd, tsConfigFile) } : {}) as never,
      (tsConfigFile ? { tsConfig: extractTSConfig(join(ctx.cwd, tsConfigFile)) } : {}) as never,
    );
    const out = result.output as unknown as { summary: { violations: DcViolation[] } };
    return out.summary.violations.map((x) => {
      const [rule, name] = x.rule.name.split(':');
      const def = cfg.architecture.forbidden.find((r, i) => String(r.name ?? i) === name);
      const msg = rule === 'ARCH-6'
        ? `imports fixture ${x.to}. Only tests and dev-only code may load fixtures.`
        : `imports ${x.to}, crossing boundary "${name}".${def?.message ? ` ${def.message}` : ''}`;
      return violation(rule, x.from, msg, `${name}:${x.to}`);
    });
  }
}

/** ARCH-2 singletons, ARCH-5 restricted strings, ARCH-6 public fixture assets referenced by prod code. */
function scanSources(ctx: CheckContext, files: string[]): Violation[] {
  const cfg = ctx.config;
  const v: Violation[] = [];
  const product = files.filter((f) => isProductFile(cfg, f) && !matchesAny(f, cfg.fixtures.devOnly));
  const texts = new Map(product.map((f) => [f, readText(ctx.cwd, f)]));

  for (const name of cfg.architecture.singletons) {
    const def = new RegExp(`\\b(?:function\\s*\\*?\\s+${name}\\b|(?:const|let|var)\\s+${name}\\s*[=:]|(?:async\\s+)?${name}\\s*\\([^)]*\\)\\s*(?::[^{=]+)?\\{)`);
    const where = product.filter((f) => def.test(texts.get(f) ?? ''));
    if (where.length > 1)
      for (const f of where.slice(1))
        v.push(violation('ARCH-2', f, `defines ${name}, which is already defined in ${where[0]}. Keep one implementation and import it.`, `singleton:${name}`));
  }

  for (const r of cfg.architecture.restrictedStrings) {
    const re = new RegExp(r.pattern);
    for (const [f, src] of texts) {
      if (matchesAny(f, r.allowIn)) continue;
      const i = src.split('\n').findIndex((l) => re.test(l));
      if (i >= 0) v.push(violation('ARCH-5', f, `matches /${r.pattern}/. ${r.message}`, `str:${r.pattern}`, i + 1));
    }
  }

  const pub = cfg.fixtures.publicDir.replace(/\/$/, '');
  const assets = files.filter((f) => f.startsWith(`${pub}/`) && matchesAny(f, cfg.fixtures.globs));
  if (assets.length) {
    const consumers = files.filter(
      (f) => !isTestFile(cfg, f) && !isHelperFile(cfg, f) && !matchesAny(f, cfg.fixtures.devOnly) &&
        (texts.has(f) || (/\.(css|scss|html|astro|vue|svelte)$/.test(f) && !f.startsWith(`${pub}/`))),
    );
    for (const a of assets) {
      const served = a.slice(pub.length);
      for (const c of consumers) {
        const src = texts.get(c) ?? readText(ctx.cwd, c);
        if (src.includes(served)) v.push(violation('ARCH-6', c, `references fixture asset ${served}. Fixtures must not ship to every user.`, `asset:${served}`));
      }
    }
  }
  return v;
}

/** ARCH-4: dependencies added vs base that are not in deps.allow. */
function newDeps(ctx: CheckContext): { v: Violation[]; added: string[] } {
  if (!ctx.base || !existsSync(join(ctx.cwd, 'package.json'))) return { v: [], added: [] };
  const old = showAt(ctx.cwd, mergeBase(ctx.cwd, ctx.base), 'package.json');
  const names = (s: string | null): Set<string> => {
    if (!s) return new Set();
    const p = JSON.parse(s);
    return new Set([...Object.keys(p.dependencies ?? {}), ...Object.keys(p.devDependencies ?? {}), ...Object.keys(p.optionalDependencies ?? {})]);
  };
  const before = names(old);
  const added = [...names(readText(ctx.cwd, 'package.json'))].filter((d) => !before.has(d));
  const v = added
    .filter((d) => !ctx.config.deps.allow.includes(d))
    .map((d) => violation('ARCH-4', 'package.json', `adds dependency "${d}" (not in deps.allow). A code owner must add it to deps.allow (CODEOWNER-protected config).`, `dep:${d}`));
  return { v, added };
}

/** ARCH-3: modules unreachable from production entries, via knip --production. */
function knipUnused(ctx: CheckContext): { v: Violation[]; error?: string } {
  const cfg = ctx.config;
  if (!cfg.architecture.entries.length) return { v: [] };
  const tmp = mkdtempSync(join(tmpdir(), 'guardrails-knip-'));
  const conf = join(tmp, 'knip.json');
  const project = cfg.sourceRoots.map((r) => (r.includes('*') ? `${r}!` : `${r.replace(/\/$/, '')}/**/*.{ts,tsx,js,jsx,mjs}!`));
  writeFileSync(conf, JSON.stringify({ entry: cfg.architecture.entries.map((e) => `${e}!`), project, ignore: [...cfg.tests.globs, ...cfg.tests.helperGlobs, ...cfg.fixtures.globs] }));
  try {
    let dir = dirname(require.resolve('knip'));
    while (!existsSync(join(dir, 'bin', 'knip.js')) && dirname(dir) !== dir) dir = dirname(dir);
    const bin = join(dir, 'bin', 'knip.js');
    const out = execFileSync(process.execPath, [bin, '--config', conf, '--production', '--include', 'files', '--reporter', 'json', '--no-exit-code', '--no-progress'], {
      cwd: ctx.cwd, encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'pipe'], timeout: 180_000,
    });
    const json = JSON.parse(out.slice(out.indexOf('{'))) as { files?: string[]; issues?: { file: string; files?: unknown[] }[] };
    const unused = new Set<string>(json.files ?? []);
    for (const i of json.issues ?? []) if (i.files && (i.files as unknown[]).length) unused.add(i.file);
    return {
      v: [...unused].filter((f) => SOURCE_EXT.test(f)).map((f) => violation('ARCH-3', f, 'is not reachable from any production entry (architecture.entries). Wire it in or delete it and its tests.', 'unreachable')),
    };
  } catch (e) {
    return { v: [], error: (e as Error).message.split('\n')[0] };
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

export async function checkArch(ctx: CheckContext, files = listFiles(ctx.cwd)): Promise<CheckOutput> {
  const metrics: Metric[] = [];
  const violations = [...(await cruiseBoundaries(ctx, files)), ...scanSources(ctx, files)];
  const deps = newDeps(ctx);
  violations.push(...deps.v);
  if (deps.added.length) metrics.push({ name: 'deps.added', value: deps.added.length, detail: deps.added.join(', ') });
  if (ctx.options.knip) {
    const k = knipUnused(ctx);
    violations.push(...k.v);
    if (k.error) metrics.push({ name: 'knip', value: 'error', detail: k.error });
  }
  return { violations, metrics };
}
