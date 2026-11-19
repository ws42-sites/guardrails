import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { listFiles, mergeBase, showAt, violation } from '../util.js';
import { KINDS, auditSuite, readSuiteBaseline, regressions, type SuiteBaseline } from '../test-suite.js';

/** SUITE-1..5: findings above the committed baseline block. SUITE-6 (diff mode): the baseline only shrinks. */
export function checkSuite(ctx: CheckContext, files = listFiles(ctx.cwd)): CheckOutput {
  const ts = ctx.config.testSuite;
  if (!ts.enabled) return { violations: [] };
  const audit = auditSuite(ctx.cwd, ctx.config, files);
  const baseline = readSuiteBaseline(ctx.cwd, ts.baselineFile);
  const v: Violation[] = regressions(audit, baseline).map((r) => violation(KINDS[r.kind].rule, r.file, r.message, r.key));
  if (!baseline && v.length) v.unshift(violation('SUITE-6', ts.baselineFile, `no test-suite baseline. Run "guardrails check-tests --update-baseline" and commit ${ts.baselineFile}.`, 'missing'));
  if (ctx.base && baseline) {
    const raw = showAt(ctx.cwd, mergeBase(ctx.cwd, ctx.base), ts.baselineFile);
    if (raw && !ctx.labels.includes(ctx.config.labels.rebaseline)) {
      const before = JSON.parse(raw) as SuiteBaseline;
      const grew = Object.entries(baseline.byKind).filter(([k, n]) => n > (before.byKind[k] ?? 0)).map(([k, n]) => `${k} ${before.byKind[k] ?? 0} -> ${n}`);
      if (grew.length) v.push(violation('SUITE-6', ts.baselineFile, `test-suite baseline grew (${grew.join(', ')}). Fix the new findings instead, or get the "${ctx.config.labels.rebaseline}" label from a code owner.`, `grew:${grew.join(',')}`));
    }
  }
  const t = audit.totals;
  return { violations: v, metrics: [{ name: 'suite.ratio', value: t.ratio, detail: `${t.testLines} test lines / ${t.productLines} product lines in ${t.testFiles} test files` }] };
}
