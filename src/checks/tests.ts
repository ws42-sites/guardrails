import { configFromSources } from '../config-md.js';
import type { CheckContext, CheckOutput, Metric, Tier, Violation } from '../types.js';
import { inventory, MATCHER_STRENGTH, matcherName, type TestCase } from '../inventory.js';
import {
  diffAgainst, filesAt, isHelperFile, isProductFile, isTestFile, lineCount, listFiles, matchesAny,
  mergeBase, readText, showAt, tierOf, violation,
} from '../util.js';
import { ruleInfo } from '../rules.js';
import { LEGACY_LABELS, hasLabel } from '../labels.js';

export interface SpecRow {
  id: string;
  tier: Tier;
  behaviour: string;
  tests: { file: string; title: string; line: number }[];
}

export interface TestIndex {
  byFile: Record<string, TestCase[]>;
  parseErrors: Record<string, string>;
}

export function indexTests(ctx: CheckContext, files = listFiles(ctx.cwd)): TestIndex {
  const byFile: Record<string, TestCase[]> = {};
  const parseErrors: Record<string, string> = {};
  for (const f of files.filter((x) => isTestFile(ctx.config, x))) {
    const inv = inventory(readText(ctx.cwd, f), ctx.config.specs.idPattern, f);
    byFile[f] = inv.tests;
    if (inv.parseError) parseErrors[f] = inv.parseError;
  }
  return { byFile, parseErrors };
}

export function specMatrix(ctx: CheckContext, index = indexTests(ctx)): SpecRow[] {
  return Object.entries(ctx.config.specs.items).map(([id, s]) => ({
    id,
    tier: s.tier,
    behaviour: s.behaviour,
    tests: Object.entries(index.byFile).flatMap(([file, ts]) =>
      ts.filter((t) => t.specId === id).map((t) => ({ file, title: t.title, line: t.line }))),
  }));
}

const MOCK_ONLY = /^(not\.)?(toHaveBeenCalled|toHaveBeenCalledTimes|toHaveBeenCalledWith|toHaveBeenLastCalledWith|toHaveBeenNthCalledWith|toHaveBeenCalledOnce)$/;
const TAUTOLOGY = (m: string): boolean => /@expect\((true|1)\)$/.test(m) || /^(toBeDefined|toBeTruthy|not\.toThrow|not\.toBeUndefined|not\.toBeNull)(@|$)/.test(m);
/** TEST-5W heuristics: only flag when EVERY assertion in the test is weak. */
export function weakness(t: TestCase): string | null {
  if (t.expects === 0) return 'no expect() calls. Assert the observable result.';
  if (t.matchers.every((m) => MOCK_ONLY.test(m.split('@')[0]))) return 'only asserts that a mock was called. Also assert the result the caller sees.';
  if (t.matchers.every(TAUTOLOGY)) return `only tautological assertions (${[...new Set(t.matchers.map((m) => m.split('@')[0]))].join(', ')}). Assert the exact value.`;
  return null;
}

const TIER_RANK: Record<Tier, number> = { T1: 3, T2: 2, T3: 1 };
const codeLines = (lines: string[]): number =>
  lines.filter((l) => l.trim() && !/^\s*(\/\/|\/\*|\*)/.test(l)).length;

/** TEST-1, TEST-2, TEST-4 (always) and TEST-3, TEST-7, BASE-1 (diff mode). */
export function checkTests(ctx: CheckContext, files = listFiles(ctx.cwd)): CheckOutput {
  const cfg = ctx.config;
  const v: Violation[] = [];
  const metrics: Metric[] = [];
  const index = indexTests(ctx, files);
  const specs = cfg.specs.items;

  for (const [file, err] of Object.entries(index.parseErrors))
    v.push(violation('TEST-1', file, `could not parse test file: ${err}`, 'parse'));

  let total = 0;
  for (const [file, tests] of Object.entries(index.byFile)) {
    total += tests.length;
    for (const t of tests) {
      if (t.malformedId) {
        v.push(violation('TEST-1', file, `"${t.title}": "${t.malformedId}" does not match ${cfg.specs.idPattern}.`, `malformed:${t.title}`, t.line));
      } else if (!t.specId) {
        const x = violation('TEST-1', file, `"${t.title}" has no spec ID. Start the title (or its describe) with an ID from guardrails.config.json specs.items, e.g. "REVIEW-03 ...".`, `missing:${t.title}`, t.line);
        x.maxMode = 'warn';
        v.push(x);
      } else if (!specs[t.specId]) {
        v.push(violation('TEST-1', file, `"${t.title}" uses ${t.specId}, which is not in specs.items. Add the behaviour to the spec block first (a code owner reviews it).`, `unknown:${t.specId}:${t.title}`, t.line));
      }
    }
    for (const t of tests) {
      const weak = weakness(t);
      if (weak) v.push(violation('TEST-5W', file, `"${t.title}": ${weak}`, `weak:${t.title}`, t.line));
    }
    const n = lineCount(readText(ctx.cwd, file));
    if (n > cfg.tests.maxFileLines)
      v.push(violation('TEST-4', file, `${n} lines (cap ${cfg.tests.maxFileLines}). Split by behaviour or use it.each.`, 'size'));
  }
  metrics.push({ name: 'tests', value: total });

  const matrix = specMatrix(ctx, index);
  for (const row of matrix) {
    if (row.tests.length === 0 && row.tier !== 'T3' && !cfg.holdout?.specs.includes(row.id)) {
      const x = violation('TEST-2', 'guardrails.config.json', `${row.id} (${row.tier}) "${row.behaviour}" has no test.`, row.id);
      if (row.tier === 'T2') x.maxMode = 'warn';
      v.push(x);
    }
    if (row.tests.length > cfg.specs.maxTestsPerId)
      v.push(violation('TEST-4', row.tests[0].file, `${row.id} has ${row.tests.length} tests (cap ${cfg.specs.maxTestsPerId}). That is padding; merge into it.each.`, `perid:${row.id}`));
  }
  metrics.push({ name: 'specs', value: matrix.length, detail: `${matrix.filter((r) => r.tests.length).length} with tests` });

  if (ctx.base) diffChecks(ctx, index, v, metrics);
  return { violations: v, metrics };
}

function diffChecks(ctx: CheckContext, index: TestIndex, v: Violation[], metrics: Metric[]): void {
  const cfg = ctx.config;
  const base = ctx.base as string;
  const mb = mergeBase(ctx.cwd, base);
  const diff = diffAgainst(ctx.cwd, base);

  // ---- TEST-3 budget, per tier, fixtures excluded ----
  const prod: Record<Tier, number> = { T1: 0, T2: 0, T3: 0 };
  const test: Record<Tier, number> = { T1: 0, T2: 0, T3: 0 };
  for (const [file, d] of Object.entries(diff)) {
    if (matchesAny(file, cfg.fixtures.globs)) continue;
    if (isTestFile(cfg, file) || isHelperFile(cfg, file)) {
      const ids = (index.byFile[file] ?? []).map((t) => t.specId).filter((x): x is string => !!x);
      const tiers = ids.map((id) => cfg.specs.items[id]?.tier).filter((t): t is Tier => !!t);
      const tier = tiers.sort((a, b) => TIER_RANK[b] - TIER_RANK[a])[0] ?? 'T2';
      test[tier] += codeLines(d.added);
    } else if (isProductFile(cfg, file)) {
      prod[tierOf(cfg, file)] += codeLines(d.added);
    }
  }
  for (const t of ['T1', 'T2', 'T3'] as Tier[]) {
    if (!test[t] && !prod[t]) continue;
    const budget = cfg.tests.budget[t];
    if (prod[t] === 0) {
      metrics.push({ name: `budget.${t}`, value: 'test-only', detail: `${test[t]} test lines, 0 product lines` });
      continue;
    }
    const ratio = test[t] / prod[t];
    metrics.push({ name: `budget.${t}`, value: Number(ratio.toFixed(2)), detail: `${test[t]} test / ${prod[t]} product lines, budget ${budget}` });
    if (ratio > budget)
      v.push(violation('TEST-3', '(diff)', `${t} test lines are ${ratio.toFixed(2)}x product lines (budget ${budget}): ${test[t]} test vs ${prod[t]} product.`, `budget:${t}`));
  }

  // ---- TEST-7 weakening ----
  const reduceOk = hasLabel(ctx.labels, cfg.tests.testsRemovedLabel, LEGACY_LABELS.testsRemoved);
  const baseTests: Record<string, TestCase[]> = {};
  for (const f of filesAt(ctx.cwd, mb).filter((x) => isTestFile(cfg, x))) {
    const src = showAt(ctx.cwd, mb, f);
    if (src) baseTests[f] = inventory(src, cfg.specs.idPattern, f).tests;
  }
  const headTitles = new Set(Object.values(index.byFile).flat().map((t) => t.title));
  const deleted: string[] = [];
  let baseCount = 0;
  for (const [file, tests] of Object.entries(baseTests)) {
    baseCount += tests.length;
    const head = index.byFile[file] ?? [];
    const nth = new Map<string, number>();
    for (const bt of tests) {
      const n = nth.get(bt.title) ?? 0;
      nth.set(bt.title, n + 1);
      const ht = head.filter((x) => x.title === bt.title)[n];
      if (!ht) {
        if (!headTitles.has(bt.title)) deleted.push(`${file}: ${bt.title}`);
        continue;
      }
      if (ht.expects < bt.expects) {
        v.push(violation('TEST-7', file, `"${bt.title}" lost ${bt.expects - ht.expects} expect() call(s). Restore them.`, `expects:${bt.title}`, ht.line));
        continue;
      }
      const strength = (ms: string[]) => ms.reduce((s, m) => s + (MATCHER_STRENGTH[matcherName(m)] ?? 2), 0);
      if (strength(ht.matchers) < strength(bt.matchers))
        v.push(violation('TEST-7', file, `"${bt.title}" loosened a matcher (${bt.matchers.join(', ')} -> ${ht.matchers.join(', ')}). Keep the stricter matcher.`, `matcher:${bt.title}`, ht.line));
    }
  }
  const headCount = Object.values(index.byFile).reduce((n, t) => n + t.length, 0);
  metrics.push({ name: 'tests.vsBase', value: headCount - baseCount, detail: `${baseCount} on base, ${headCount} now` });
  if (headCount < baseCount)
    v.push(violation('TEST-10', '(diff)', `test count dropped by ${baseCount - headCount} (${baseCount} on base, ${headCount} now).`, 'drop'));
  if (deleted.length) {
    metrics.push({ name: 'tests.deletedFromBase', value: deleted.length, detail: deleted.slice(0, 50).join('\n') });
    if (!reduceOk)
      for (const d of deleted)
        v.push(violation('TEST-7', d.split(': ')[0], `deleted base test "${d.slice(d.indexOf(': ') + 2)}". Deleting base tests needs a code owner to add the "${cfg.tests.testsRemovedLabel}" label, which approves removing tests.`, `deleted:${d}`));
  }

  configWeakening(ctx, mb, v);
}

type Json = null | boolean | number | string | Json[] | { [k: string]: Json };
const MODE_RANK: Record<string, number> = { off: 0, warn: 1, block: 2 };
/** Keys where a bigger number is looser. Everything else numeric: smaller is looser. */
const LOOSER_WHEN_BIGGER = /(^|\.)(budget\.T\d|maxFileLines|maxTestsPerId|maxLines\.[^.]+|timeoutMinutes|maxPercent|minLines|minTokens|maxProductLines|propertyTimeLimitMs|cacheHours|runbook\.maxLines|testSuite\.maxLines)$/;
const LOOSER_WHEN_LIST_GROWS = /(^|\.)(deps\.allow|docs\.allow|docs\.ignore|runbook\.allowDocs|allowIn|except|fixtures\.devOnly|excludedMutations|mutation\.exclude|cliEntries|testEnvAllow|holdout\.specs|testSuite\.exclude)$/;
const LOOSER_WHEN_LIST_SHRINKS = /(^|\.)(tiers\.T1|tiers\.T2|forbidden|restrictedStrings|singletons|sourceRoots|tests\.globs|docs\.extensions|docs\.disguise|docs\.pointerFiles|lock\.paths)$/;

export function compareConfigs(before: Json, after: Json, path = ''): string[] {
  const out: string[] = [];
  if (typeof after === 'boolean' && (before === undefined || typeof before === 'boolean')) {
    if (/(^|\.)(enabled|specTests)$/.test(path) && before !== false && after === false) out.push(`${path}: disabled`);
    if (/\.regressionGuard$/.test(path) && after && !before) out.push(`${path}: set (tests may pass on base)`);
    return out;
  }
  if (typeof before === 'number' && typeof after === 'number') {
    if (LOOSER_WHEN_BIGGER.test(path) ? after > before : after < before) out.push(`${path}: ${before} -> ${after}`);
    return out;
  }
  if (typeof before === 'string' && typeof after === 'string' && path.startsWith('rules.')) {
    if ((MODE_RANK[after] ?? 2) < (MODE_RANK[before] ?? 2)) out.push(`${path}: ${before} -> ${after}`);
    return out;
  }
  if (path.startsWith('specs.items.') && path.endsWith('.tier') && typeof before === 'string' && typeof after === 'string') {
    if (after > before) out.push(`${path}: ${before} -> ${after}`);
    return out;
  }
  if (Array.isArray(before) && Array.isArray(after)) {
    const b = new Set(before.map((x) => JSON.stringify(x)));
    const a = new Set(after.map((x) => JSON.stringify(x)));
    if (LOOSER_WHEN_LIST_GROWS.test(path)) for (const x of a) if (!b.has(x)) out.push(`${path}: added ${x}`);
    if (LOOSER_WHEN_LIST_SHRINKS.test(path)) for (const x of b) if (!a.has(x)) out.push(`${path}: removed ${x}`);
    return out;
  }
  if (before && after && typeof before === 'object' && typeof after === 'object' && !Array.isArray(before) && !Array.isArray(after)) {
    for (const k of Object.keys(before)) {
      const p = path ? `${path}.${k}` : k;
      if (k in after) { if (path !== 'journeys.items') out.push(...compareConfigs(before[k], after[k], p)); }
      else if (path !== 'specs.items' && path !== 'journeys.items' && path !== 'rules' && k !== '$schema') out.push(`${p}: removed`);
    }
    for (const k of Object.keys(after)) {
      // Adopting the runbook standard (absent = disabled) can only tighten.
      if (k in before || path === 'rules' || path.startsWith('journeys.items') || (path === '' && (k === 'runbook' || k === 'testSuite'))) continue;
      const p = path ? `${path}.${k}` : k;
      const x = after[k];
      if (typeof x === 'boolean') out.push(...compareConfigs(undefined as unknown as Json, x, p));
      else if (Array.isArray(x)) out.push(...compareConfigs([], x, p));
      else if (x && typeof x === 'object') out.push(...compareConfigs({}, x, p));
    }
    if (path === 'rules')
      for (const k of Object.keys(after))
        if (!(k in before) && (MODE_RANK[String(after[k])] ?? 2) < (MODE_RANK[ruleInfo(k)?.mode ?? 'block'] ?? 2))
          out.push(`rules.${k}: set to ${String(after[k])}`);
  }
  return out;
}

const THRESHOLD_FILES = /^(vitest|vite|stryker)\.conf(ig)?\.(ts|js|mjs|cjs|json|mts)$|^stryker\.conf\.(js|json)$/;
const thresholdNumbers = (src: string): Record<string, number> => {
  const out: Record<string, number> = {};
  for (const m of src.matchAll(/["']?(break|high|low|lines|branches|functions|statements|perFile)["']?\s*:\s*(\d+(?:\.\d+)?)/g)) out[m[1]] = Number(m[2]);
  return out;
};

function configWeakening(ctx: CheckContext, mb: string, v: Violation[]): void {
  const configFiles = ['guardrails.md', 'guardrails.config.json'];
  const before = configFiles.map((f) => showAt(ctx.cwd, mb, f));
  const after = configFiles.map((f) => { try { return readText(ctx.cwd, f); } catch { return null; } });
  if (before.some((s) => s !== null)) {
    try {
      const changes = compareConfigs(configFromSources(before[0], before[1]) as unknown as Json, configFromSources(after[0], after[1]) as unknown as Json);
      const approved = ctx.labels.includes(ctx.config.labels.configApproved);
      for (const file of configFiles.filter((_, i) => before[i] !== after[i]))
        for (const c of changes)
          if (!approved) v.push(violation('TEST-7', file, `config loosened: ${c}. Lowering a threshold counts as weakening; a code owner must add the "${ctx.config.labels.configApproved}" label, which approves loosening ${file}.`, `cfg:${c}`));
    } catch {}
  }
  for (const f of listFiles(ctx.cwd).filter((x) => THRESHOLD_FILES.test(x))) {
    const old = showAt(ctx.cwd, mb, f);
    if (!old) continue;
    const a = thresholdNumbers(old);
    const b = thresholdNumbers(readText(ctx.cwd, f));
    for (const [k, n] of Object.entries(a))
      if (k in b && b[k] < n) v.push(violation('TEST-7', f, `threshold ${k} lowered ${n} -> ${b[k]}.`, `thr:${k}`));
  }
  const bl = ctx.config.baselineFile;
  const oldBl = showAt(ctx.cwd, mb, bl);
  const newBl = (() => { try { return readText(ctx.cwd, bl); } catch { return null; } })();
  // The adoption PR (no guardrails config on base yet) records the initial baseline; growth after that needs the label.
  if (newBl && before.some((s) => s !== null)) {
    const keys = (s: string | null) => new Set<string>(s ? (JSON.parse(s).entries ?? []).map((e: { rule: string; fingerprint: string }) => `${e.rule}:${e.fingerprint}`) : []);
    const o = keys(oldBl);
    const added = [...keys(newBl)].filter((k) => !o.has(k));
    if (added.length && !hasLabel(ctx.labels, ctx.config.labels.rebaseline, LEGACY_LABELS.baseline))
      v.push(violation('BASE-1', bl, `baseline grew by ${added.length} entr${added.length === 1 ? 'y' : 'ies'}. Fix the new violations instead, or have a code owner add the "${ctx.config.labels.rebaseline}" label, which approves letting the baseline grow.`, `grew:${added.sort().join(',')}`));
  }
}
