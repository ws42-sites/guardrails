import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CLOCK_SHIFT_MODULE, checkClock } from '../src/checks/clock.js';
import type { Metric, Violation } from '../src/types.js';
import { fingerprint } from '../src/util.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

interface StepSpec { code: number; json?: unknown; rawJson?: string; stderr?: string; stdout?: string }
interface RunLog { argv: string[]; shiftEnv: string | null; nodeOptions: string | null; activeShiftMs: number; tempDirExists: boolean | null; outputFile: string | null }

const RUNNER_SOURCE = [
  "import fs from 'node:fs';",
  "import path from 'node:path';",
  'const cwd = process.cwd();',
  "const spec = JSON.parse(fs.readFileSync(path.join(cwd, 'scenario.json'), 'utf8'));",
  "const logPath = path.join(cwd, 'runs.json');",
  "const runs = fs.existsSync(logPath) ? JSON.parse(fs.readFileSync(logPath, 'utf8')) : [];",
  'const step = runs.length === 0 ? spec.shifted : spec.normal;',
  'const argv = process.argv.slice(2);',
  "const outArg = argv.find((a) => a.startsWith('--outputFile='));",
  "const outputFile = outArg ? outArg.slice('--outputFile='.length) : null;",
  'runs.push({ argv, shiftEnv: process.env.GUARDRAILS_CLOCK_SHIFT_YEARS ?? null, nodeOptions: process.env.NODE_OPTIONS ?? null, activeShiftMs: globalThis.__guardrailsClockShift ?? 0, tempDirExists: outputFile ? fs.existsSync(path.dirname(outputFile)) : null, outputFile });',
  'fs.writeFileSync(logPath, JSON.stringify(runs));',
  'if (outputFile && step.rawJson !== undefined) fs.writeFileSync(outputFile, step.rawJson);',
  "else if (outputFile && step.json !== undefined && step.json !== null) fs.writeFileSync(outputFile, JSON.stringify(JSON.parse(JSON.stringify(step.json).replaceAll('$CWD', JSON.stringify(cwd).slice(1, -1)))));",
  'if (step.stdout) process.stdout.write(step.stdout);',
  'if (step.stderr) process.stderr.write(step.stderr);',
  'process.exit(step.code);',
].join('\n');

const shiftViol = (file: string, name: string, years: number, rawKey = `${file} > ${name}`): Violation => ({
  rule: 'TEST-6', file, line: undefined,
  message: `"${name}" passes today but fails with the clock at +${years}y. Freeze time with vi.useFakeTimers() and vi.setSystemTime().`,
  fingerprint: fingerprint('TEST-6', file, `clock:${rawKey}`),
});

const suiteViol = (years: number, tail: string): Violation => ({
  rule: 'TEST-6', file: '(suite)', line: undefined,
  message: `suite fails only with the clock at +${years}y: ${tail}`,
  fingerprint: fingerprint('TEST-6', '(suite)', 'suite'),
});

function setupClockRepo(scriptName: string, years: number, shifted: StepSpec, normal?: StepSpec) {
  const r = repo(
    { [scriptName]: RUNNER_SOURCE, 'scenario.json': JSON.stringify({ shifted, normal: normal ?? { code: 0, json: {} } }) },
    { ...BASE_CONFIG, tests: { command: `node ${scriptName}`, clockShiftYears: years } },
  );
  return { r, readRuns: (): RunLog[] => JSON.parse(readFileSync(join(r.dir, 'runs.json'), 'utf8')) as RunLog[] };
}

describe('CLOCK-02 checkClock detects future-only failures and cleans up', () => {
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z')); });
  afterEach(() => { vi.useRealTimers(); });

  it.each<{ label: string; years: number; shifted: StepSpec; normal?: StepSpec; violations: Violation[]; metrics: Metric[]; count: number }>([
    {
      label: 'shifted green passes without running normal suite',
      years: 3, count: 1, violations: [], metrics: [{ name: 'clockShift', value: 'pass', detail: '+3y' }],
      shifted: { code: 0, json: { testResults: [{ name: '$CWD/src/ok.test.ts', assertionResults: [{ fullName: 'CORE-01 ok', status: 'passed' }] }] } },
    },
    {
      label: 'reports only future-failing tests and filters already-broken or passing tests',
      years: 2, count: 2,
      shifted: {
        code: 1,
        json: {
          testResults: [
            { name: '$CWD/src/empty.test.ts' },
            { name: '$CWD/src/bomb.test.ts', assertionResults: [
              { fullName: '  SUITE > CORE-01 bomb  ', status: 'failed' },
              { fullName: 'CORE-01 already broken', status: 'failed' },
              { fullName: 'CORE-01 safe', status: 'passed' },
              { fullName: 'CORE-01 skipped', status: 'skipped' },
            ] },
            { name: '$CWD/src/second.test.ts', assertionResults: [{ fullName: 'LIB-01 second bomb', status: 'failed' }] },
          ],
        },
      },
      normal: {
        code: 0,
        json: { testResults: [{ name: '$CWD/src/bomb.test.ts', assertionResults: [
          { fullName: '  SUITE > CORE-01 bomb  ', status: 'passed' },
          { fullName: 'CORE-01 already broken', status: 'failed' },
          { fullName: 'CORE-01 safe', status: 'passed' },
        ] }] },
      },
      violations: [
        shiftViol('src/bomb.test.ts', 'SUITE > CORE-01 bomb', 2, 'src/bomb.test.ts >   SUITE > CORE-01 bomb  '),
        shiftViol('src/second.test.ts', 'LIB-01 second bomb', 2),
      ],
      metrics: [{ name: 'clockShift', value: 'fail', detail: undefined }],
    },
    {
      label: 'reports future-only failure alongside inconclusive detail when normal also exits non-zero',
      years: 2, count: 2,
      shifted: { code: 1, json: { testResults: [{ name: '$CWD/src/mixed.test.ts', assertionResults: [{ fullName: 'CORE-01 future', status: 'failed' }, { fullName: 'CORE-01 red', status: 'failed' }] }] } },
      normal: { code: 1, json: { testResults: [{ name: '$CWD/src/mixed.test.ts', assertionResults: [{ fullName: 'CORE-01 red', status: 'failed' }] }] } },
      violations: [shiftViol('src/mixed.test.ts', 'CORE-01 future', 2)],
      metrics: [{ name: 'clockShift', value: 'fail', detail: 'suite also fails without shift; TEST-6 inconclusive' }],
    },
    {
      label: 'suite red both ways with identical failures returns suite-red',
      years: 2, count: 2,
      shifted: { code: 1, json: { testResults: [{ name: '$CWD/src/red.test.ts', assertionResults: [{ fullName: 'CORE-01 red', status: 'failed' }] }] } },
      normal: { code: 2, json: { testResults: [{ name: '$CWD/src/red.test.ts', assertionResults: [{ fullName: 'CORE-01 red', status: 'failed' }] }] } },
      violations: [],
      metrics: [{ name: 'clockShift', value: 'suite-red', detail: 'suite also fails without shift; TEST-6 inconclusive' }],
    },
    {
      label: 'vitest failure without output json falls back to suite violation with last 5 stderr lines',
      years: 4, count: 2,
      shifted: { code: 1, json: null, stderr: 'e1\ne2\ne3\ne4\ne5\ne6\ne7', stdout: 'ignored-stdout' },
      normal: { code: 0, json: {} },
      violations: [suiteViol(4, 'e3 e4 e5 e6 e7')],
      metrics: [{ name: 'clockShift', value: 'fail', detail: undefined }],
    },
  ])('CLOCK-02 evaluates vitest runs: $label', ({ years, shifted, normal, violations, metrics, count }) => {
    const { r, readRuns } = setupClockRepo('fake-vitest.mjs', years, shifted, normal);
    expect(checkClock(r.ctx())).toEqual({ violations, metrics });
    const runs = readRuns();
    expect(runs).toHaveLength(count);
    const firstOut = runs[0].outputFile ?? '';
    expect([runs[0].argv, runs[0].shiftEnv, runs[0].activeShiftMs, runs[0].tempDirExists, basename(firstOut), basename(dirname(firstOut)).startsWith('guardrails-clock-'), existsSync(dirname(firstOut))]).toEqual([
      ['--reporter=json', `--outputFile=${firstOut}`], String(years), Math.round(years * 365.25 * 864e5), true, 'shifted.json', true, false,
    ]);
    if (count === 2) {
      const secondOut = runs[1].outputFile ?? '';
      expect([runs[1].argv, runs[1].shiftEnv, runs[1].activeShiftMs, basename(secondOut), dirname(secondOut)]).toEqual([
        ['--reporter=json', `--outputFile=${secondOut}`], '', 0, 'normal.json', dirname(firstOut),
      ]);
    }
  });

  it.each<{ label: string; scriptName: string; shifted: StepSpec; normal: StepSpec; violations: Violation[]; metrics: Metric[] }>([
    { label: 'non-vitest command uses stdout tail when stderr is empty', scriptName: 'runner.mjs', shifted: { code: 1, stderr: '', stdout: 'o1\no2\no3\no4\no5\no6' }, normal: { code: 0 }, violations: [suiteViol(2, 'o2 o3 o4 o5 o6')], metrics: [{ name: 'clockShift', value: 'fail', detail: undefined }] },
    { label: 'prefix boundary notvitest.mjs is not treated as vitest', scriptName: 'notvitest.mjs', shifted: { code: 1, stderr: 'prefix-err', stdout: '' }, normal: { code: 0 }, violations: [suiteViol(2, 'prefix-err')], metrics: [{ name: 'clockShift', value: 'fail', detail: undefined }] },
    { label: 'suffix boundary vitestx.mjs is not treated as vitest', scriptName: 'vitestx.mjs', shifted: { code: 1, stderr: 'suffix-err', stdout: '' }, normal: { code: 0 }, violations: [suiteViol(2, 'suffix-err')], metrics: [{ name: 'clockShift', value: 'fail', detail: undefined }] },
    { label: 'non-vitest command red both ways reports suite-red', scriptName: 'runner.mjs', shifted: { code: 1, stderr: 'always-red' }, normal: { code: 1, stderr: 'always-red' }, violations: [], metrics: [{ name: 'clockShift', value: 'suite-red', detail: 'suite also fails without shift; TEST-6 inconclusive' }] },
  ])('CLOCK-02 evaluates non-vitest commands and regex boundaries: $label', ({ scriptName, shifted, normal, violations, metrics }) => {
    const { r, readRuns } = setupClockRepo(scriptName, 2, shifted, normal);
    expect(checkClock(r.ctx())).toEqual({ violations, metrics });
    expect(readRuns().map((x) => x.argv)).toEqual([[], []]);
  });

  it('CLOCK-02 strips inherited clock-shift NODE_OPTIONS, handles unset NODE_OPTIONS, and removes temp dir on error', () => {
    const importFlag = `--import=${pathToFileURL(CLOCK_SHIFT_MODULE).href}`;
    expect([CLOCK_SHIFT_MODULE.endsWith(join('src', 'clock-shift.mjs')), existsSync(CLOCK_SHIFT_MODULE)]).toEqual([true, true]);
    const prevNodeOpts = process.env.NODE_OPTIONS;
    const prevYears = process.env.GUARDRAILS_CLOCK_SHIFT_YEARS;
    try {
      delete process.env.NODE_OPTIONS;
      process.env.GUARDRAILS_CLOCK_SHIFT_YEARS = '7';
      const unsetCase = setupClockRepo('fake-vitest.mjs', 2, { code: 1, json: {} }, { code: 0, json: {} });
      checkClock(unsetCase.r.ctx());
      expect(unsetCase.readRuns().map((x) => [x.nodeOptions, x.shiftEnv, x.activeShiftMs])).toEqual([
        [importFlag, '2', Math.round(2 * 365.25 * 864e5)], ['', '', 0],
      ]);

      process.env.NODE_OPTIONS = `  --no-warnings --import=file:///stale/clock-shift.mjs ${importFlag}  `;
      const stripCase = setupClockRepo('fake-vitest.mjs', 2, { code: 1, json: {} }, { code: 0, json: {} });
      checkClock(stripCase.r.ctx());
      expect(stripCase.readRuns().map((x) => [x.nodeOptions, x.shiftEnv, x.activeShiftMs])).toEqual([
        [`--no-warnings ${importFlag}`, '2', Math.round(2 * 365.25 * 864e5)], ['--no-warnings', '', 0],
      ]);

      delete process.env.NODE_OPTIONS;
      const throwCase = setupClockRepo('fake-vitest.mjs', 2, { code: 1, rawJson: '{broken' }, { code: 0, json: {} });
      expect(() => checkClock(throwCase.r.ctx())).toThrow(SyntaxError);
      const crashedOut = throwCase.readRuns()[0].outputFile ?? '';
      expect([crashedOut.length > 0, existsSync(dirname(crashedOut))]).toEqual([true, false]);
    } finally {
      if (prevNodeOpts === undefined) delete process.env.NODE_OPTIONS; else process.env.NODE_OPTIONS = prevNodeOpts;
      if (prevYears === undefined) delete process.env.GUARDRAILS_CLOCK_SHIFT_YEARS; else process.env.GUARDRAILS_CLOCK_SHIFT_YEARS = prevYears;
    }
  });
});
