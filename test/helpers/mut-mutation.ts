import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach } from 'vitest';
import { parseConfig } from '../../src/config.js';
import type { SurvivingMutant } from '../../src/checks/mutation.js';
import type { CheckContext, CheckOutput, Violation } from '../../src/types.js';
import { fingerprint } from '../../src/util.js';
import { BASE_CONFIG } from './repo.js';

export const CANARY = 'CANARY-hmut-7b4e91a2';

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
