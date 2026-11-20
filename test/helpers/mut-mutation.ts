import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach } from 'vitest';
import type { SurvivingMutant } from '../../src/checks/mutation.js';
import { parseConfig } from '../../src/config.js';
import type { CheckContext, CheckOutput, Violation } from '../../src/types.js';
import { fingerprint } from '../../src/util.js';
import { BASE_CONFIG } from './repo.js';

export const CANARY = 'CANARY-hmut-7b4e91a2';
export const HOSTILE_TEXT = 'false\r\n\u0000\u2028<script>x</script>\'; DROP TABLE m;--{"nested":1}https://user:pass@xn--pple-43d.example/p';
export const LONG_TEXT = 'L'.repeat(4096);

const tempDirs: string[] = [];
afterEach(() => {
  while (tempDirs.length) rmSync(tempDirs.pop() as string, { recursive: true, force: true });
});

export interface MutantInput {
  status: string;
  mutatorName?: string;
  replacement?: string;
  location?: { start: { line: number } };
}

export function checkoutBranch(dir: string, branch: string): void {
  execFileSync('git', ['checkout', '-q', '-b', branch], { cwd: dir, stdio: 'pipe' });
}

export function installStryker(dir: string): void {
  mkdirSync(join(dir, 'node_modules', '@stryker-mutator', 'core'), { recursive: true });
}

export function mutConfig(mutation: Record<string, unknown> = {}) {
  return { ...BASE_CONFIG, mutation };
}

export function plainCtx(files: Record<string, string>, mutation: Record<string, unknown> = {}): CheckContext {
  const cwd = mkdtempSync(join(tmpdir(), 'guardrails-mut-ctx-'));
  tempDirs.push(cwd);
  for (const [rel, body] of Object.entries(files)) {
    mkdirSync(dirname(join(cwd, rel)), { recursive: true });
    writeFileSync(join(cwd, rel), body);
  }
  const config = parseConfig(mutConfig(mutation));
  return { cwd, config, base: null, labels: [], options: { clockShift: false, mutation: false, knip: false } };
}

export function scoreViolation(file: string, scoreText: string, breakPct: number, survivingCount: number, validCount: number): Violation {
  return {
    rule: 'TEST-8',
    file,
    line: undefined,
    message: `mutation score ${scoreText}% is below ${breakPct}% (${survivingCount} surviving/uncovered of ${validCount}). Add assertions that kill them.`,
    fingerprint: fingerprint('TEST-8', file, 'score'),
  };
}

export function multiFileScoreReport(cwd: string): string {
  return JSON.stringify({
    files: {
      [`${cwd}/src/core/a.ts`]: {
        mutants: [
          { status: 'Killed' }, { status: 'Killed' }, { status: 'Timeout' },
          { status: 'Survived', replacement: CANARY }, { status: 'NoCoverage', replacement: CANARY },
          { status: 'Ignored', replacement: CANARY }, { status: 'CompileError' }, { status: 'RuntimeError' },
        ],
      },
      'src/core/b.ts': {
        mutants: [
          { status: 'Killed' }, { status: 'Killed' }, { status: 'Killed' },
          { status: 'Killed' }, { status: 'Timeout' }, { status: 'Survived' },
        ],
      },
      'src/core/zero.ts': { mutants: [{ status: 'Ignored' }, { status: 'CompileError' }, { status: 'RuntimeError' }] },
      'src/core/stale.ts': { mutants: [{ status: 'Survived', replacement: CANARY }] },
      [`${cwd}x/src/core/a.ts`]: { mutants: [{ status: 'Survived', replacement: CANARY }] },
    },
  });
}

export function mixedSurvivorsReport(cwd: string): string {
  const base = JSON.stringify({
    'src/core/a.ts': {
      mutants: [
        { status: 'Survived', mutatorName: 'ConditionalExpression', replacement: HOSTILE_TEXT, location: { start: { line: 17 } } },
        { status: 'NoCoverage' },
        { status: 'NoCoverage', mutatorName: 'StringLiteral', replacement: LONG_TEXT, location: { start: { line: 51 } } },
        { status: 'Killed', mutatorName: 'K', replacement: CANARY, location: { start: { line: 3 } } },
        { status: 'Timeout', mutatorName: 'T', replacement: CANARY, location: { start: { line: 4 } } },
        { status: 'Ignored', mutatorName: 'I', replacement: CANARY, location: { start: { line: 5 } } },
        { status: 'CompileError', mutatorName: 'C', replacement: CANARY, location: { start: { line: 6 } } },
        { status: 'RuntimeError', mutatorName: 'R', replacement: CANARY, location: { start: { line: 7 } } },
      ],
    },
    [`${cwd}/src/core/b.ts`]: { mutants: [{ status: 'Survived', mutatorName: '', replacement: 'x || y', location: { start: { line: 29 } } }] },
    'src/core/stale.ts': { mutants: [{ status: 'Survived', mutatorName: 'S', replacement: CANARY, location: { start: { line: 99 } } }] },
    [`${cwd}x/src/core/b.ts`]: { mutants: [{ status: 'NoCoverage', mutatorName: 'N', replacement: CANARY, location: { start: { line: 88 } } }] },
  });
  return `{"files":${base.slice(0, -1)},"__proto__":{"mutants":[{"status":"Survived","replacement":"${CANARY}"}]}}}`;
}

export interface SeedRow {
  idx: number;
  breakPct: number;
  mutants: MutantInput[];
  expectedCheck: CheckOutput;
  expectedSurvivors: SurvivingMutant[];
}

export function buildSeededRows(): SeedRow[] {
  let state = 133742;
  const nextInt = (mod: number) => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state % mod;
  };
  const rows: SeedRow[] = [];
  for (let idx = 0; idx < 8; idx++) {
    const killed = 2 + nextInt(4);
    const timeout = nextInt(3);
    const survived = nextInt(3);
    const noCov = nextInt(3);
    const ignored = nextInt(2);
    const breakPct = 50 + nextInt(45);
    const mutants: MutantInput[] = [
      ...Array.from({ length: killed }, (_, i) => ({ status: 'Killed', mutatorName: 'Arith', replacement: CANARY, location: { start: { line: i + 1 } } })),
      ...Array.from({ length: timeout }, (_, i) => ({ status: 'Timeout', mutatorName: 'Loop', replacement: CANARY, location: { start: { line: i + 10 } } })),
      ...Array.from({ length: survived }, (_, i) => ({ status: 'Survived', mutatorName: `Mut${i}`, replacement: `rep-${i}`, location: { start: { line: i + 20 } } })),
      ...Array.from({ length: noCov }, (_, i) => ({ status: 'NoCoverage', mutatorName: `Cov${i}`, replacement: `cov-${i}`, location: { start: { line: i + 30 } } })),
      ...Array.from({ length: ignored }, () => ({ status: 'Ignored', replacement: CANARY })),
    ];
    const k = killed + timeout;
    const n = k + survived + noCov;
    const rawScore = (100 * k) / n;
    const fixed = rawScore.toFixed(1);
    const violations = rawScore < breakPct ? [scoreViolation('src/core/seed.ts', fixed, breakPct, n - k, n)] : [];
    const expectedSurvivors: SurvivingMutant[] = [
      ...Array.from({ length: survived }, (_, i) => ({ file: 'src/core/seed.ts', line: i + 20, mutator: `Mut${i}`, replacement: `rep-${i}`, status: 'Survived' })),
      ...Array.from({ length: noCov }, (_, i) => ({ file: 'src/core/seed.ts', line: i + 30, mutator: `Cov${i}`, replacement: `cov-${i}`, status: 'NoCoverage' })),
    ];
    rows.push({
      idx,
      breakPct,
      mutants,
      expectedCheck: { violations, metrics: [{ name: 'mutation', value: Number(fixed), detail: `src/core/seed.ts: ${fixed}% (${k}/${n})` }] },
      expectedSurvivors,
    });
  }
  return rows;
}
