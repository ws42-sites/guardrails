import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { diffAgainst, isProductFile, violation } from '../util.js';
import { runCommand } from './run.js';

interface Side { name: string; start: number; end: number }
interface Clone { firstFile: Side; secondFile: Side }

function jscpdBin(): string {
  const req = createRequire(import.meta.url);
  let dir = dirname(req.resolve('jscpd'));
  while (!existsSync(join(dir, 'package.json'))) dir = dirname(dir);
  return join(dir, 'bin', 'jscpd');
}

/** ARCH-8: copy-paste in the lines this PR adds (Sonar's new-code duplication gate). */
export function checkDuplication(ctx: CheckContext, files: string[]): CheckOutput {
  const d = ctx.config.duplication;
  if (!ctx.base || !d.enabled) return { violations: [] };
  const diff = diffAgainst(ctx.cwd, ctx.base);
  const added = new Map<string, Set<number>>();
  for (const [f, fd] of Object.entries(diff))
    if (fd.status !== 'deleted' && isProductFile(ctx.config, f) && /\.(m|c)?[jt]sx?$/.test(f)) added.set(f, new Set(fd.addedLines.filter((n, i) => fd.added[i].trim() !== '')));
  const newLines = [...added.values()].reduce((n, s) => n + s.size, 0);
  if (!newLines) return { violations: [] };

  const product = files.filter((f) => isProductFile(ctx.config, f) && /\.(m|c)?[jt]sx?$/.test(f));
  const out = mkdtempSync(join(tmpdir(), 'guardrails-jscpd-'));
  const v: Violation[] = [];
  try {
    const r = runCommand(
      `node ${JSON.stringify(jscpdBin())} --min-lines ${d.minLines} --min-tokens ${d.minTokens} --reporters json --output ${JSON.stringify(out)} --silent --absolute ${product.map((f) => JSON.stringify(f)).join(' ')}`,
      ctx.cwd,
    );
    const report = join(out, 'jscpd-report.json');
    if (!existsSync(report)) return { violations: [], metrics: [{ name: 'duplication', value: 'error', detail: r.stderr.split('\n')[0] }] };
    const clones = (JSON.parse(readFileSync(report, 'utf8')) as { duplicates: Clone[] }).duplicates;
    const rel = (n: string) => relative(ctx.cwd, resolve(ctx.cwd, n)).split('\\').join('/');
    const dup = new Map<string, Set<number>>();
    const pairs = new Map<string, string>();
    for (const c of clones)
      for (const [a, b] of [[c.firstFile, c.secondFile], [c.secondFile, c.firstFile]] as const) {
        const f = rel(a.name);
        const lines = added.get(f);
        if (!lines) continue;
        for (let n = a.start; n <= a.end; n++) if (lines.has(n)) {
          if (!dup.has(f)) dup.set(f, new Set());
          dup.get(f)?.add(n);
          pairs.set(f, `${rel(b.name)}:${b.start}`);
        }
      }
    const dupLines = [...dup.values()].reduce((n, s) => n + s.size, 0);
    const pct = Math.round((dupLines / newLines) * 1000) / 10;
    if (pct > d.maxPercent)
      for (const [f, s] of dup)
        v.push(violation('ARCH-8', f, `${s.size} added lines are copies of ${pairs.get(f)} (${pct}% of new product lines duplicated; max ${d.maxPercent}%). Extract and reuse a shared function.`, `dup:${f}`, Math.min(...s)));
    return { violations: v, metrics: [{ name: 'duplication.newCode', value: `${pct}%`, detail: `${dupLines} of ${newLines} added product lines duplicated` }] };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
