import { loadConfig, ConfigError } from './config.js';
import { readBaseline, key } from './baseline.js';
import { RULES, ruleInfo } from './rules.js';
import type { CheckContext, Finding, JourneyRow, Metric, Mode, Report, Violation } from './types.js';
import { listFiles, violation } from './util.js';
import { checkDocs } from './checks/docs.js';
import { checkSetup } from './checks/setup.js';
import { checkTests } from './checks/tests.js';
import { checkLint } from './checks/lint.js';
import { checkArch } from './checks/arch.js';
import { checkClock } from './checks/clock.js';
import { checkMutation } from './checks/mutation.js';
import { checkGaming } from './checks/gaming.js';
import { checkDuplication } from './checks/dup.js';
import { checkScope } from './checks/scope.js';
import { checkFailsOnBase } from './checks/failbase.js';
import { checkFlags } from './flags.js';
import { checkJourneys } from './checks/journeys.js';
import { checkRunbook } from './checks/runbook.js';
import { checkCi } from './checks/ci.js';
import { checkSuite } from './checks/suite.js';

export interface RunOptions {
  cwd: string;
  base?: string | null;
  labels?: string[];
  clockShift?: boolean;
  mutation?: boolean;
  knip?: boolean;
  only?: string[];
  /** Restrict TEST-8 to files matching these globs. */
  mutate?: string[];
  /** Run TEST-11: new/changed tests must fail on the base branch. */
  failsOnBase?: boolean;
  /** Spec IDs this task implements (SCOPE-1). Default: IDs in the changed tests. */
  specs?: string[];
}

/** Rules a baseline may contain. Diff-only rules and runtime rules are never baselined. */
export const BASELINEABLE = new Set(RULES.filter((r) => !r.diffOnly && !['TEST-6', 'TEST-8', 'TEST-9', 'CFG-1', 'CFG-2', 'FLAG-1', 'HOLD-1', 'SEC-1', 'JRN-5', 'SUITE-1', 'SUITE-2', 'SUITE-3', 'SUITE-4', 'SUITE-5', 'SUITE-6'].includes(r.id)).map((r) => r.id));

/** Rules `guardrails baseline` records: baselineable and not switched off in this config. */
export function baselineRules(overrides: Record<string, Mode>): Set<string> {
  return new Set([...BASELINEABLE].filter((id) => effectiveMode(id, overrides) !== 'off'));
}

const RANK: Record<Mode, number> = { off: 0, warn: 1, block: 2 };

export function effectiveMode(rule: string, overrides: Record<string, Mode>, cap?: Mode): Mode {
  const m = overrides[rule] ?? ruleInfo(rule)?.mode ?? 'block';
  return cap && RANK[cap] < RANK[m] ? cap : m;
}

export async function collect(opts: RunOptions): Promise<{ ctx: CheckContext; violations: Violation[]; metrics: Metric[]; journeys?: JourneyRow[] }> {
  const config = loadConfig(opts.cwd);
  const ctx: CheckContext = {
    cwd: opts.cwd,
    config,
    base: opts.base ?? null,
    labels: opts.labels ?? [],
    options: { clockShift: !!opts.clockShift, mutation: !!opts.mutation, knip: opts.knip !== false, mutate: opts.mutate, failsOnBase: !!opts.failsOnBase, specs: opts.specs },
  };
  const files = listFiles(opts.cwd);
  const want = (g: string) => !opts.only || opts.only.includes(g);
  const outs = [];
  if (want('docs')) outs.push(checkDocs(ctx, files));
  if (want('runbook')) outs.push(checkRunbook(ctx, files));
  if (want('ci')) outs.push(checkCi(ctx, files));
  if (want('suite')) outs.push(checkSuite(ctx, files));
  if (want('setup')) outs.push(checkSetup(ctx, files));
  if (want('tests')) outs.push(checkTests(ctx, files));
  if (want('lint')) outs.push(await checkLint(ctx, files));
  if (want('arch')) outs.push(await checkArch(ctx, files), checkDuplication(ctx, files));
  if (want('gaming')) outs.push(checkGaming(ctx, files));
  if (want('flags')) outs.push(checkFlags(ctx));
  if (want('scope')) outs.push(checkScope(ctx));
  if (want('journeys')) outs.push(checkJourneys(ctx, files));
  if (want('failbase') && ctx.options.failsOnBase) outs.push(checkFailsOnBase(ctx));
  if (want('clock') && ctx.options.clockShift) outs.push(checkClock(ctx));
  if (want('mutation') && ctx.options.mutation) outs.push(checkMutation(ctx));
  return { ctx, violations: outs.flatMap((o) => o.violations), metrics: outs.flatMap((o) => o.metrics ?? []), journeys: outs.find((o) => o.journeys)?.journeys };
}

export async function run(opts: RunOptions): Promise<Report> {
  let collected;
  try {
    collected = await collect(opts);
  } catch (e) {
    if (!(e instanceof ConfigError)) throw e;
    const f: Finding = { ...violation('CFG-1', 'guardrails.config.json', e.message, 'cfg'), mode: 'block', known: false };
    return { ok: false, base: opts.base ?? null, findings: [f], metrics: [], counts: { block: 1, warn: 0, known: 0 }, staleBaseline: 0 };
  }
  const { ctx, violations, metrics, journeys } = collected;
  const baseline = readBaseline(ctx.cwd, ctx.config.baselineFile);
  const known = new Set(baseline.entries.map(key));
  const seen = new Set<string>();
  const findings: Finding[] = [];
  for (const v of violations) {
    const mode = effectiveMode(v.rule, ctx.config.rules, v.maxMode);
    if (mode === 'off') continue;
    const k = key(v);
    seen.add(k);
    findings.push({ ...v, mode, known: BASELINEABLE.has(v.rule) && known.has(k) });
  }
  const staleBaseline = opts.only ? 0 : baseline.entries.filter((e) => !seen.has(key(e))).length;
  const counts = {
    block: findings.filter((f) => f.mode === 'block' && !f.known).length,
    warn: findings.filter((f) => f.mode === 'warn' && !f.known).length,
    known: findings.filter((f) => f.known).length,
  };
  return { ok: counts.block === 0, base: ctx.base, findings, metrics, counts, staleBaseline, ...(journeys?.length ? { journeys } : {}) };
}
