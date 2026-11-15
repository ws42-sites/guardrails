import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import type { CheckContext, CheckOutput, Metric, Violation } from '../types.js';
import { inventory, type TestCase } from '../inventory.js';
import { diffAgainst, git, isHelperFile, isTestFile, mergeBase, readText, showAt, violation } from '../util.js';
import { runCommand } from './run.js';

interface Assertion { title: string; status: string }
interface VitestJson { testResults?: { name: string; assertionResults?: Assertion[] }[] }

/** Vitest reports it.each titles with values filled in; turn an inventory title into a matcher. */
export function titleMatcher(title: string): RegExp {
  const esc = title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`^${esc.replace(/%[sdifjoO#%]|\\\$\\\{\\\}|\\\$[a-zA-Z_.]+/g, '.*')}$`);
}

function results(file: string, cwd: string): Map<string, Assertion[]> {
  const out = new Map<string, Assertion[]>();
  if (!existsSync(file)) return out;
  const j = JSON.parse(readFileSync(file, 'utf8')) as VitestJson;
  for (const r of j.testResults ?? []) {
    const rel = r.name.startsWith(cwd) ? r.name.slice(cwd.length + 1) : r.name;
    out.set(rel.split('\\').join('/'), r.assertionResults ?? []);
  }
  return out;
}

/** Tests that are new, or whose source changed, versus the merge-base. */
export function changedTests(ctx: CheckContext, mb: string, files: string[]): { file: string; test: TestCase }[] {
  const out: { file: string; test: TestCase }[] = [];
  for (const f of files) {
    const head = inventory(readText(ctx.cwd, f), ctx.config.specs.idPattern, f).tests;
    const old = showAt(ctx.cwd, mb, f);
    const base = old ? inventory(old, ctx.config.specs.idPattern, f).tests : [];
    for (const t of head) {
      const b = base.find((x) => x.title === t.title);
      if (!b || b.hash !== t.hash) out.push({ file: f, test: t });
    }
  }
  return out;
}

/**
 * TEST-11: run the PR's new/changed tests against the base branch's product code. They must fail
 * there (else they prove nothing about this change) and pass on head.
 */
export function checkFailsOnBase(ctx: CheckContext): CheckOutput {
  if (!ctx.base) return { violations: [] };
  const cfg = ctx.config;
  const mb = mergeBase(ctx.cwd, ctx.base);
  const diff = diffAgainst(ctx.cwd, ctx.base);
  const testFiles = Object.entries(diff).filter(([f, d]) => d.status !== 'deleted' && isTestFile(cfg, f)).map(([f]) => f);
  const helpers = Object.entries(diff).filter(([f, d]) => d.status !== 'deleted' && isHelperFile(cfg, f)).map(([f]) => f);
  const targets = changedTests(ctx, mb, testFiles);
  if (!targets.length) return { violations: [], metrics: [{ name: 'failsOnBase', value: 'skipped', detail: 'no new or changed tests' }] };

  const tmp = mkdtempSync(join(tmpdir(), 'guardrails-base-'));
  const wt = join(tmp, 'base');
  const v: Violation[] = [];
  const metrics: Metric[] = [];
  try {
    git(ctx.cwd, ['worktree', 'add', '--detach', '--force', wt, mb]);
    if (existsSync(join(ctx.cwd, 'node_modules'))) symlinkSync(join(ctx.cwd, 'node_modules'), join(wt, 'node_modules'), 'junction');
    for (const f of [...new Set([...testFiles, ...helpers, 'guardrails.config.json'])]) {
      if (!existsSync(join(ctx.cwd, f))) continue;
      mkdirSync(dirname(join(wt, f)), { recursive: true });
      cpSync(join(ctx.cwd, f), join(wt, f));
    }
    const files = [...new Set(targets.map((t) => t.file))];
    const run = (dir: string, name: string) => {
      const out = join(tmp, `${name}.json`);
      runCommand(`${cfg.tests.command} --reporter=json --outputFile=${JSON.stringify(out)} ${files.map((f) => JSON.stringify(f)).join(' ')}`, dir, { GUARDRAILS_CLOCK_SHIFT_YEARS: '' });
      return results(out, dir);
    };
    const onBase = run(wt, 'base');
    const onHead = run(ctx.cwd, 'head');
    const characterization = ctx.labels.includes(cfg.labels.characterization);
    let failedOnBase = 0;
    let guards = 0;
    for (const { file, test } of targets) {
      const m = titleMatcher(test.title);
      const baseRes = (onBase.get(file) ?? []).filter((a) => m.test(a.title));
      const headRes = (onHead.get(file) ?? []).filter((a) => m.test(a.title));
      const headPass = headRes.length > 0 && headRes.every((a) => a.status === 'passed');
      const basePass = baseRes.length > 0 && baseRes.every((a) => a.status === 'passed');
      if (!headPass) {
        v.push(violation('TEST-11', file, `"${test.title}" does not pass on this branch.`, `head:${test.title}`, test.line));
        continue;
      }
      if (!basePass) {
        failedOnBase++;
        continue;
      }
      const spec = test.specId ? cfg.specs.items[test.specId] : undefined;
      if (spec?.regressionGuard || characterization) {
        guards++;
        continue;
      }
      v.push(violation('TEST-11', file, `"${test.title}" already passes on ${ctx.base} without this change, so it does not test the change. Assert the new behaviour, or mark ${test.specId ?? 'the spec'} regressionGuard / use the "${cfg.labels.characterization}" label for characterization tests.`, `base:${test.title}`, test.line));
    }
    metrics.push({ name: 'failsOnBase', value: `${failedOnBase}/${targets.length}`, detail: `${failedOnBase} new/changed tests fail on base as required${guards ? `, ${guards} allowed as regression guards/characterization` : ''}` });
  } catch (e) {
    metrics.push({ name: 'failsOnBase', value: 'error', detail: (e as Error).message.split('\n')[0] });
  } finally {
    try { git(ctx.cwd, ['worktree', 'remove', '--force', wt]); } catch { /* already gone */ }
    rmSync(tmp, { recursive: true, force: true });
  }
  return { violations: v, metrics };
}
