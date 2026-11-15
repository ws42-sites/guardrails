import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative } from 'node:path';
import type { GuardrailsConfig } from './config.js';
import type { Violation } from './types.js';
import { globToRegex, violation } from './util.js';
import { runCommand } from './checks/run.js';

function walk(dir: string, root = dir): string[] {
  const out: string[] = [];
  for (const e of readdirSync(dir)) {
    if (e === 'node_modules' || e === '.git') continue;
    const p = join(dir, e);
    if (statSync(p).isDirectory()) out.push(...walk(p, root));
    else out.push(relative(root, p).split('\\').join('/'));
  }
  return out;
}

/**
 * HOLD-1: tests the building agent never saw. They live in a separate private checkout and only
 * run at the PR gate. Only titles and pass/fail are reported, never test source.
 */
export function runHoldout(cwd: string, cfg: GuardrailsConfig, dir: string): { violations: Violation[]; ran: number } {
  const h = cfg.holdout;
  if (!h) return { violations: [violation('HOLD-1', 'guardrails.config.json', 'holdout is not configured.', 'nocfg')], ran: 0 };
  const src = h.path ? join(dir, h.path) : dir;
  if (!existsSync(src)) return { violations: [violation('HOLD-1', '(holdout)', `holdout checkout not found at ${src}.`, 'missing')], ran: 0 };
  const isTest = (f: string) => cfg.tests.globs.some((g) => globToRegex(g).test(f) || globToRegex(g).test(`x/${f}`));
  const files = walk(src);
  const tests = files.filter(isTest);
  const target = join(cwd, h.target);
  rmSync(target, { recursive: true, force: true });
  for (const f of files) {
    mkdirSync(dirname(join(target, f)), { recursive: true });
    cpSync(join(src, f), join(target, f));
  }
  const tmp = mkdtempSync(join(tmpdir(), 'guardrails-holdout-'));
  const out = join(tmp, 'r.json');
  const v: Violation[] = [];
  try {
    const rel = tests.map((f) => `${h.target}/${f}`);
    if (!rel.length) return { violations: [violation('HOLD-1', '(holdout)', 'no holdout tests found.', 'empty')], ran: 0 };
    const r = runCommand(`${cfg.tests.command} --reporter=json --outputFile=${JSON.stringify(out)} ${rel.map((f) => JSON.stringify(f)).join(' ')}`, cwd, { GUARDRAILS_CLOCK_SHIFT_YEARS: '' });
    if (!existsSync(out)) return { violations: [violation('HOLD-1', '(holdout)', `holdout run produced no report (exit ${r.code}).`, 'noreport')], ran: 0 };
    const j = JSON.parse(readFileSync(out, 'utf8')) as { testResults?: { assertionResults?: { title: string; fullName?: string; status: string }[] }[] };
    let ran = 0;
    for (const res of j.testResults ?? [])
      for (const a of res.assertionResults ?? []) {
        ran++;
        const name = (a.fullName ?? a.title).trim();
        if (a.status !== 'passed' && a.status !== 'skipped' && a.status !== 'pending')
          v.push(violation('HOLD-1', '(holdout)', `hidden test failed: "${name}". Re-read the spec; the hidden tests check the behaviour, not your tests.`, `fail:${name}`));
      }
    if (j.testResults?.length === 0 || ran === 0) v.push(violation('HOLD-1', '(holdout)', 'no holdout tests ran.', 'none'));
    return { violations: v, ran };
  } finally {
    rmSync(tmp, { recursive: true, force: true });
    rmSync(target, { recursive: true, force: true });
  }
}
