import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { violation } from '../util.js';
import { runCommand } from './run.js';

const here = dirname(fileURLToPath(import.meta.url));
export const CLOCK_SHIFT_MODULE = join(here, '..', 'clock-shift.mjs');

interface VitestJson { testResults?: { name: string; assertionResults?: { fullName: string; status: string }[] }[] }

function failures(file: string, cwd: string): Set<string> {
  const out = new Set<string>();
  if (!existsSync(file)) return out;
  const json = JSON.parse(readFileSync(file, 'utf8')) as VitestJson;
  for (const r of json.testResults ?? [])
    for (const a of r.assertionResults ?? [])
      if (a.status === 'failed') out.add(`${r.name.replace(`${cwd}/`, '')} > ${a.fullName}`);
  return out;
}

/** TEST-6: run the suite with the clock at now + N years; anything that only fails shifted is a time bomb. */
export function checkClock(ctx: CheckContext): CheckOutput {
  const { command, clockShiftYears } = ctx.config.tests;
  const dir = mkdtempSync(join(tmpdir(), 'guardrails-clock-'));
  const vitest = /\bvitest\b/.test(command);
  const run = (shift: boolean, name: string) => {
    const json = join(dir, `${name}.json`);
    const cmd = vitest ? `${command} --reporter=json --outputFile=${JSON.stringify(json)}` : command;
    // Strip any inherited shift so the "normal" run really is normal (e.g. when guardrails tests itself).
    const inherited = (process.env.NODE_OPTIONS ?? '').replace(/--import=\S*clock-shift\.mjs/g, '').trim();
    const env: Record<string, string> = shift
      ? { GUARDRAILS_CLOCK_SHIFT_YEARS: String(clockShiftYears), NODE_OPTIONS: `${inherited} --import=${pathToFileURL(CLOCK_SHIFT_MODULE).href}`.trim() }
      : { GUARDRAILS_CLOCK_SHIFT_YEARS: '', NODE_OPTIONS: inherited };
    const r = runCommand(cmd, ctx.cwd, env);
    return { code: r.code, failed: vitest ? failures(json, ctx.cwd) : new Set<string>(), tail: (r.stderr || r.stdout).split('\n').slice(-5).join(' ') };
  };
  try {
    const shifted = run(true, 'shifted');
    if (shifted.code === 0) return { violations: [], metrics: [{ name: 'clockShift', value: 'pass', detail: `+${clockShiftYears}y` }] };
    const normal = run(false, 'normal');
    const v: Violation[] = [];
    const onlyShifted = [...shifted.failed].filter((t) => !normal.failed.has(t));
    for (const t of onlyShifted) {
      const [file] = t.split(' > ');
      const name = t.slice(file.length + 3).trim();
      v.push(violation('TEST-6', file, `"${name}" passes today but fails with the clock at +${clockShiftYears}y. Freeze time with vi.useFakeTimers() and vi.setSystemTime().`, `clock:${t}`));
    }
    if (!onlyShifted.length && normal.code === 0)
      v.push(violation('TEST-6', '(suite)', `suite fails only with the clock at +${clockShiftYears}y: ${shifted.tail}`, 'suite'));
    return { violations: v, metrics: [{ name: 'clockShift', value: v.length ? 'fail' : 'suite-red', detail: normal.code ? 'suite also fails without shift; TEST-6 inconclusive' : undefined }] };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
