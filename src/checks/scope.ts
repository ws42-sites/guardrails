import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { diffAgainst, isProductFile, isTestFile, matchesAny, violation } from '../util.js';

/** SCOPE-1 (warn): PRs that sprawl beyond their task. */
export function checkScope(ctx: CheckContext): CheckOutput {
  if (!ctx.base) return { violations: [] };
  const cfg = ctx.config;
  const diff = diffAgainst(ctx.cwd, ctx.base);
  const v: Violation[] = [];
  let lines = 0;
  const product: string[] = [];
  for (const [f, d] of Object.entries(diff)) {
    if (!isProductFile(cfg, f)) continue;
    product.push(f);
    lines += d.added.length + d.removed.length;
  }
  if (lines > cfg.scope.maxProductLines)
    v.push(violation('SCOPE-1', '(diff)', `${lines} product lines changed (cap ${cfg.scope.maxProductLines}). Split the PR so each part can be reviewed.`, 'lines'));

  let ids = ctx.options.specs ?? [];
  if (!ids.length) {
    const re = new RegExp(cfg.specs.idPattern.replace(/^\^|\$$/g, ''), 'g');
    ids = [...new Set(Object.entries(diff).filter(([f]) => isTestFile(cfg, f)).flatMap(([, d]) => d.added.join('\n').match(re) ?? []))].filter((id) => cfg.specs.items[id]);
  }
  const paths = ids.flatMap((id) => cfg.specs.items[id]?.paths ?? []);
  if (paths.length)
    for (const f of product)
      if (!matchesAny(f, paths))
        v.push(violation('SCOPE-1', f, `changed outside the paths of this task's specs (${ids.join(', ')}). Keep the PR to the task or flag the spec.`, 'outside'));
  return { violations: v.map((x) => ({ ...x, maxMode: 'warn' as const })), metrics: [{ name: 'scope.productLines', value: lines, detail: ids.length ? `specs: ${ids.join(', ')}` : undefined }] };
}
