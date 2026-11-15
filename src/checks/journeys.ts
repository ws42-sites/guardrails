import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { GuardrailsConfig } from '../config.js';
import type { CheckContext, CheckOutput, JourneyRow, Metric, Violation } from '../types.js';
import { diffAgainst, isE2eFile, isProductFile, listFiles, matchesAny, mergeBase, readText, showAt, violation } from '../util.js';
import { indexTests } from './tests.js';

type Journey = GuardrailsConfig['journeys']['items'][string];

const idSource = (cfg: GuardrailsConfig) => cfg.journeys.idPattern.replace(/^\^|\$$/g, '');

/** Journey e2e tests: test()/test.describe()/it()/describe() calls whose title starts with a journey ID. */
export function e2eTests(cfg: GuardrailsConfig, cwd: string, files: string[]): Map<string, { file: string; line: number }[]> {
  const re = new RegExp(`\\b(?:test|it|describe)(?:\\.(?:describe|only|skip|fixme|slow|serial))*\\(\\s*(['"\`])(${idSource(cfg)})\\b`, 'g');
  const out = new Map<string, { file: string; line: number }[]>();
  for (const f of files.filter((x) => isE2eFile(cfg, x))) {
    const src = readText(cwd, f);
    for (const m of src.matchAll(re)) {
      const line = src.slice(0, m.index).split('\n').length;
      out.set(m[2], [...(out.get(m[2]) ?? []), { file: f, line }]);
    }
  }
  return out;
}

interface PwSpec { title: string; ok: boolean }
interface PwSuite { title: string; specs?: PwSpec[]; suites?: PwSuite[] }
/** Pass/fail per journey ID from a Playwright JSON report. */
export function e2eResults(cfg: GuardrailsConfig, cwd: string): Map<string, boolean> {
  const out = new Map<string, boolean>();
  const file = cfg.journeys.resultsFile;
  if (!file || !existsSync(join(cwd, file))) return out;
  const re = new RegExp(idSource(cfg));
  const walk = (s: PwSuite, path: string) => {
    for (const sp of s.specs ?? []) {
      const id = re.exec(`${path} ${sp.title}`)?.[0];
      if (id) out.set(id, (out.get(id) ?? true) && sp.ok);
    }
    for (const c of s.suites ?? []) walk(c, `${path} ${c.title}`);
  };
  for (const s of (JSON.parse(readFileSync(join(cwd, file), 'utf8')) as { suites?: PwSuite[] }).suites ?? []) walk(s, s.title);
  return out;
}

/** Journey → specs → tests matrix, with e2e status. */
export function journeyMatrix(ctx: CheckContext, files = listFiles(ctx.cwd)): JourneyRow[] {
  const cfg = ctx.config;
  const index = indexTests(ctx, files);
  const e2e = e2eTests(cfg, ctx.cwd, files);
  const results = e2eResults(cfg, ctx.cwd);
  const testsFor = (spec: string) => Object.values(index.byFile).flat().filter((t) => t.specId === spec).length;
  return Object.entries(cfg.journeys.items).map(([id, j]) => {
    const tests = e2e.get(id) ?? [];
    const result = results.get(id);
    const status: JourneyRow['status'] = !j.approved ? 'draft' : !tests.length ? 'missing' : result === undefined ? 'covered' : result ? 'pass' : 'fail';
    return { id, title: j.title, priority: j.priority, approved: j.approved, specs: j.specs.map((s) => ({ id: s, tests: testsFor(s) })), e2e: tests.map((t) => `${t.file}:${t.line}`), status };
  });
}

/** Turn a route file into its URL: src/pages/blog/[slug].tsx -> /blog/:slug, functions/api/x.ts -> /api/x. */
export function routeOf(file: string): string {
  let p = file.replace(/^.*?(?:^|\/)(?:pages|app|functions|routes)\//, '/').replace(/\.[cm]?[jt]sx?$|\.astro$|\.vue$|\.svelte$/, '');
  p = p.replace(/\/(index|page|route|_middleware)$/, '').replace(/\[\[?\.\.\.(\w+)\]\]?/g, '*').replace(/\[(\w+)\]/g, ':$1');
  return p || '/';
}
const routeRe = (r: string) => new RegExp(`^${r.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/:\w+/g, '[^/]+').replace(/\*/g, '.*')}/?$`);
export const routeCovered = (route: string, journeys: Journey[]) => journeys.some((j) => j.routes.some((r) => routeRe(r).test(route) || routeRe(route).test(r)));

const ROUTE_LINES = [
  /<Route\b[^>]*\bpath=["'`{]+([^"'`}]+)/,
  /\bpath:\s*["'`](\/[^"'`]*)["'`]/,
  /\b(?:app|router|server|api)\.(?:get|post|put|patch|delete|all)\(\s*["'`](\/[^"'`]*)/,
];

export function checkJourneys(ctx: CheckContext, files = listFiles(ctx.cwd)): CheckOutput {
  const cfg = ctx.config;
  const items = cfg.journeys.items;
  const v: Violation[] = [];
  const metrics: Metric[] = [];
  if (!Object.keys(items).length) return { violations: [] };
  const matrix = journeyMatrix(ctx, files);
  const approved = Object.entries(items).filter(([, j]) => j.approved);

  // JRN-1: exactly one e2e test per approved P0/P1 journey.
  for (const row of matrix) {
    if (!row.approved || row.priority === 'P2' || row.e2e.length === 1) continue;
    const x = violation('JRN-1', row.e2e[0]?.split(':')[0] ?? 'guardrails.config.json',
      row.e2e.length ? `${row.id} has ${row.e2e.length} e2e tests (${row.e2e.join(', ')}); keep exactly one journey test and put variations in steps.` : `${row.id} (${row.priority}) "${row.title}" has no e2e test titled with its ID in ${cfg.journeys.e2eGlobs.join(', ')}.`,
      `${row.id}:${row.e2e.length ? 'many' : 'none'}`);
    if (row.priority === 'P1') x.maxMode = 'warn';
    v.push(x);
  }
  for (const row of matrix.filter((r) => r.status === 'fail'))
    v.push(violation('JRN-1', row.e2e[0].split(':')[0], `${row.id} e2e test failed in ${cfg.journeys.resultsFile}.`, `${row.id}:fail`));

  // JRN-2: orphan specs (only once at least one journey is approved).
  if (approved.length) {
    const served = new Set(approved.flatMap(([, j]) => j.specs));
    for (const id of Object.keys(cfg.specs.items))
      if (!served.has(id)) v.push({ ...violation('JRN-2', 'guardrails.config.json', `${id} belongs to no approved journey. Orphan specs signal padding or unrequested features: link it from a journey's specs, or drop it.`, id), maxMode: 'warn' });
  }

  if (ctx.base) {
    // JRN-3: journeys are human-owned; any change in a PR needs a code owner.
    const before = showAt(ctx.cwd, mergeBase(ctx.cwd, ctx.base), 'guardrails.config.json');
    if (before && !ctx.labels.includes(cfg.labels.configApproved)) {
      let old: Record<string, Journey> = {};
      try { old = (JSON.parse(before).journeys?.items ?? {}) as Record<string, Journey>; } catch { /* CFG-1 */ }
      const head = (JSON.parse(readText(ctx.cwd, 'guardrails.config.json')).journeys?.items ?? {}) as Record<string, Journey>;
      for (const id of new Set([...Object.keys(old), ...Object.keys(head)])) {
        const a = old[id];
        const b = head[id];
        const what = !a ? 'added' : !b ? 'removed' : !a.approved && b.approved ? 'approved' : JSON.stringify(a) !== JSON.stringify(b) ? 'changed' : null;
        if (what) v.push(violation('JRN-3', 'guardrails.config.json', `journey ${id} ${what} in this PR. Journeys are human-owned: a code owner adds the "${cfg.labels.configApproved}" label.`, `${id}:${what}`));
      }
    }
    // JRN-4: new routes, forms and endpoints need a journey.
    const all = Object.values(items);
    const seen = new Set<string>();
    for (const [file, d] of Object.entries(diffAgainst(ctx.cwd, ctx.base))) {
      if (d.status === 'deleted' || !isProductFile(cfg, file)) continue;
      const routes: string[] = [];
      const isRouteFile = matchesAny(file, cfg.journeys.routeGlobs);
      if (isRouteFile && d.status === 'added') routes.push(routeOf(file));
      for (const line of d.added) for (const re of ROUTE_LINES) { const m = re.exec(line); if (m) routes.push(m[1]); }
      for (const r of routes)
        if (!seen.has(r) && !routeCovered(r, all)) {
          seen.add(r);
          v.push({ ...violation('JRN-4', file, `adds route ${r}, which no journey covers. Ask the owner to add it to a journey's routes (or confirm it is internal).`, `route:${r}`), maxMode: 'warn' });
        }
      if (d.added.some((l) => /<form\b/i.test(l)) && !(isRouteFile && routeCovered(routeOf(file), all)))
        v.push({ ...violation('JRN-4', file, `adds a form${isRouteFile ? ` on ${routeOf(file)}` : ''} that no journey's routes cover. Forms are user journeys; make sure one covers it.`, 'form'), maxMode: 'warn' });
    }
  }

  // JRN-5: visual diff per step (VIS-1 v2). Stub.
  if (cfg.journeys.visual.enabled) metrics.push({ name: 'journeys.visual', value: 'not implemented', detail: 'JRN-5 per-step screenshot diff arrives with VIS-1 v2; screenshots are not compared yet' });
  const gated = matrix.filter((r) => r.approved);
  metrics.push({ name: 'journeys', value: `${gated.filter((r) => r.e2e.length).length}/${gated.length}`, detail: `approved journeys with an e2e test (${matrix.length - gated.length} drafts)` });
  return { violations: v, metrics, journeys: matrix };
}
