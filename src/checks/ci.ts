import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { listFiles, matchesAny, readText, violation } from '../util.js';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

/** A test command that runs the whole suite (no change scoping). */
const FULL_TEST = /\b(npm (run )?test(:ci|:full|:all)?|npm run -s test|npx vitest run|vitest run|npx playwright test|playwright test|node --test)\b/;
const SCOPED = /--changed|--related|related\b|--only-changed|affected|test:changed|verify:changed|--grep|--since|--last-failed/;
const GUARDED = /if:.*(schedule|workflow_dispatch|event_name\s*!=\s*'pull_request'|github\.event_name\s*==\s*'push'|refs\/heads\/main)/;

/** CI-1: PRs run only affected tests; the full suite runs nightly and before deploy. */
export function checkCi(ctx: CheckContext, files = listFiles(ctx.cwd)): CheckOutput {
  const ci = ctx.config.ci;
  if (!ci.enabled) return { violations: [] };
  const v: Violation[] = [];
  let nightly = false;
  for (const f of files.filter((x) => matchesAny(x, ci.workflows))) {
    const text = readText(ctx.cwd, f);
    const lines = text.split('\n');
    const onPr = /^\s*pull_request(_target)?\s*:?/m.test(text);
    const scheduled = /^\s*schedule\s*:/m.test(text);
    lines.forEach((l, i) => {
      const code = l.replace(/(^|\s)#.*$/, '');
      if (!FULL_TEST.test(code) || SCOPED.test(code)) return;
      if (scheduled) nightly = true;
      const guarded = lines.slice(Math.max(0, i - 6), i).some((p) => GUARDED.test(p));
      if (onPr && !guarded) v.push(violation('CI-1', f, `runs the full suite on every PR ("${code.trim().slice(0, 60)}"). Run affected tests (npm run test:changed) on PRs and the full suite nightly.`, `full:${code.trim()}`, i + 1));
    });
  }
  if (!nightly && !ci.na.nightly) v.push(violation('CI-1', '.github/workflows', 'no scheduled (nightly) workflow runs the full suite. Add one that runs npm run test:full on a cron, or set ci.na.nightly with the reason.', 'nightly'));

  const pkgPath = join(ctx.cwd, ctx.config.runbook.packageJson);
  const scripts: Record<string, string> = existsSync(pkgPath) ? (JSON.parse(readText(ctx.cwd, ctx.config.runbook.packageJson)).scripts ?? {}) : {};
  for (const s of ['deploy', 'deploy:preview'] as const) {
    if (ctx.config.runbook.na[s] || ci.na.preDeploy || !(s in scripts)) continue;
    const pre = scripts[`pre${s}`] ?? '';
    if (!FULL_TEST.test(pre) || SCOPED.test(pre)) v.push(violation('CI-1', ctx.config.runbook.packageJson, `"${s}" has no full-suite safety net. Add "pre${s}": "npm run test:full" (npm runs it before ${s}).`, `pre:${s}`));
  }
  return { violations: v };
}
