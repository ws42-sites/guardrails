import { mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { checkDuplication } from '../src/checks/dup.js';
import type { CheckContext, CheckOutput, Metric, Violation } from '../src/types.js';
import { fingerprint, listFiles } from '../src/util.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const blockJs = (name: string): string[] => [
  `export function ${name}(items) {`,
  '  let sum = 0; // CANARY-9f3a7c1e',
  '  for (const x of items) {',
  '    const v = x * 2;',
  '    sum += v - 1;',
  '  }',
  '  const out = sum + 1;',
  '  return out > 0 ? out : 0;',
  '}',
];

const blockTsx = (name: string): string[] => [
  `export function ${name}(words: string[], limit: number): string[] {`,
  '  const kept: string[] = [];',
  '  for (const raw of words) {',
  '    const clean = raw.trim().toLowerCase();',
  '    if (clean.length >= limit) kept.push(clean);',
  '  }',
  '  kept.sort((left, right) => left.localeCompare(right));',
  '  return kept.slice(0, limit);',
  '}',
];

const blockSix = (name: string): string[] => [
  `export const ${name} = (a: number, b: number, c: number, d: number, e: number) => {`,
  '  const p1 = (a + b) * (c - d) + Math.max(a, b, c, d, e);',
  '  const p2 = (b + c) * (d - e) + Math.min(a, b, c, d, e);',
  '  const p3 = (p1 + p2) * (a + c + e) - (b + d);',
  '  return p1 > p2 ? p3 + p1 : p3 - p2;',
  '};',
];

const blockUnique = (name: string): string[] => [
  `export function ${name}(records: Map<string, boolean>, prefix: string): number {`,
  '  let matched = 0;',
  '  for (const [key, active] of records.entries()) {',
  '    if (active && key.startsWith(prefix)) matched += 1;',
  '    if (!active && key.endsWith(prefix)) matched -= 1;',
  '  }',
  '  return matched === 0 ? -1 : matched;',
  '}',
];

const dupViol = (file: string, count: number, source: string, pct: number, max: number, line: number): Violation => ({
  rule: 'ARCH-8',
  file,
  line,
  message: `${count} added lines are copies of ${source} (${pct}% of new product lines duplicated; max ${max}%). Extract and reuse a shared function.`,
  fingerprint: fingerprint('ARCH-8', file, `dup:${file}`),
});

function checkWithIsolatedTmp(ctx: CheckContext, files: string[]): { out: CheckOutput; leaked: string[] } {
  const scopeDir = mkdtempSync(join(tmpdir(), 'mut-dup-scope-'));
  const prev = { TMPDIR: process.env.TMPDIR, TMP: process.env.TMP, TEMP: process.env.TEMP };
  process.env.TMPDIR = scopeDir;
  process.env.TMP = scopeDir;
  process.env.TEMP = scopeDir;
  try {
    const out = checkDuplication(ctx, files);
    return { out, leaked: readdirSync(scopeDir) };
  } finally {
    for (const [k, v] of Object.entries(prev)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
    rmSync(scopeDir, { recursive: true, force: true });
  }
}

describe('DUP-02 checkDuplication gates new duplicated product code', () => {
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z')); });
  afterEach(() => { vi.useRealTimers(); });

  it.each<{ label: string; mode: 'no-base' | 'disabled' | 'no-product-lines' }>([
    { label: 'base ref is null', mode: 'no-base' },
    { label: 'duplication check is disabled in config', mode: 'disabled' },
    { label: 'diff has only deleted product files, blank lines, non-code, tests, helpers, and fixtures', mode: 'no-product-lines' },
  ])('DUP-02 skips jscpd when $label', ({ mode }) => {
    if (mode === 'no-base') {
      const r = repo({ 'src/core/a.ts': `${blockTsx('one').join('\n')}\n` }).commit();
      r.write({ 'src/core/b.ts': `${blockTsx('two').join('\n')}\n` });
      expect(checkDuplication(r.ctx(null), ['src/core/a.ts', 'src/core/b.ts'])).toEqual({ violations: [] });
      return;
    }
    if (mode === 'disabled') {
      const r = repo({ 'src/core/a.ts': `${blockTsx('one').join('\n')}\n` }, { ...BASE_CONFIG, duplication: { enabled: false } }).commit();
      r.write({ 'src/core/b.ts': `${blockTsx('two').join('\n')}\n` });
      expect(checkDuplication(r.ctx('main'), ['src/core/a.ts', 'src/core/b.ts'])).toEqual({ violations: [] });
      return;
    }
    const r = repo({ 'src/core/gone.ts': `${blockTsx('gone').join('\n')}\n`, 'src/core/blank.ts': 'export const seed = 1;\n' }).commit();
    const copyPair = `${blockTsx('dupOne').join('\n')}\n${blockTsx('dupTwo').join('\n')}\n`;
    r.remove('src/core/gone.ts').write({
      'src/core/blank.ts': 'export const seed = 1;\n   \t  \n\n',
      'src/core/data.json': '{"ok":true}\n',
      'src/core/dup.test.ts': copyPair,
      'src/fixtures/dup.ts': copyPair,
      'test/helpers/dup.ts': copyPair,
      'e2e/dup.spec.ts': copyPair,
    });
    expect(checkDuplication(r.ctx('main'), listFiles(r.dir))).toEqual({ violations: [] });
  });

  it('DUP-02 batches extensions, partial hunks, minLines, minTokens, exclusions, and cleanup', () => {
    const r = repo(
      {
        'src/core/z-base.cjs': `${blockJs('alphaBase').join('\n')}\n`,
        'src/lib/a-base.tsx': `${blockTsx('betaBase').join('\n')}\n`,
        'src/lib/z-partial.tsx': `${blockTsx('betaPartial').slice(0, 2).join('\n')}\n  return words.slice(0, limit).concat(kept); }\n`,
        'src/core/six-base.ts': `${blockSix('sixBase').join('\n')}\n`,
      },
      { ...BASE_CONFIG, duplication: { minLines: 7, minTokens: 30, maxPercent: 48.4 } },
    ).commit();

    r.write({
      'src/core/a-new.cjs': `   \n\t\n${blockJs('alphaCopy').join('\n')}\n`,
      'src/lib/z-partial.tsx': `${blockTsx('betaPartial').join('\n')}\n   \nexport const tagOne = 1;\nexport const tagTwo = 2;\nexport const tagThree = 'CANARY-9f3a7c1e';\n`,
      'src/core/six-new.ts': `${blockSix('sixCopy').join('\n')}\n`,
      'src/core/unique.ts': `${blockUnique('gammaProd').join('\n')}\n`,
      'src/fixtures/fixture-copy.ts': `${blockUnique('gammaFixture').join('\n')}\n`,
      'src/core/unit.test.ts': `${blockUnique('gammaTest').join('\n')}\n`,
    });

    const { out, leaked } = checkWithIsolatedTmp(r.ctx('main'), listFiles(r.dir));
    expect(leaked).toEqual([]);
    expect(JSON.stringify(out)).not.toContain('CANARY-9f3a7c1e');
    expect({ ...out, violations: [...out.violations].sort((a, b) => a.file.localeCompare(b.file)) }).toEqual({
      violations: [
        dupViol('src/core/a-new.cjs', 9, 'src/core/z-base.cjs:1', 48.5, 48.4, 3),
        dupViol('src/lib/z-partial.tsx', 7, 'src/lib/a-base.tsx:1', 48.5, 48.4, 3),
      ],
      metrics: [{ name: 'duplication.newCode', value: '48.5%', detail: '16 of 33 added product lines duplicated' }],
    });
  });

  it.each<{ label: string; maxPercent: number; metrics: Metric[] }>([
    {
      label: 'pct below maxPercent (45 < 45.1) passes',
      maxPercent: 45.1,
      metrics: [{ name: 'duplication.newCode', value: '45%', detail: '9 of 20 added product lines duplicated' }],
    },
    {
      label: 'pct equal to maxPercent (45 === 45) passes at exact boundary',
      maxPercent: 45,
      metrics: [{ name: 'duplication.newCode', value: '45%', detail: '9 of 20 added product lines duplicated' }],
    },
  ])('DUP-02 enforces maxPercent boundary: $label', ({ maxPercent, metrics }) => {
    const r = repo(
      { 'src/core/base.mts': `${blockTsx('baseFn').join('\n')}\n` },
      { ...BASE_CONFIG, duplication: { maxPercent } },
    ).commit();
    const extra = Array.from({ length: 11 }, (_, i) => `export const uniqueVal${i} = ${i};`).join('\n');
    r.write({
      'src/core/copy.mts': `${blockTsx('copyFn').join('\n')}\n`,
      'src/core/extra.mts': `${extra}\n`,
    });
    expect(checkDuplication(r.ctx('main'), ['src/core/base.mts', 'src/core/copy.mts', 'src/core/extra.mts'])).toEqual({
      violations: [],
      metrics,
    });
  });

  it('DUP-02 returns error metric with first stderr line when jscpd writes no report and cleans up', () => {
    const r = repo({}, { ...BASE_CONFIG, sourceRoots: ['src', '--bad'] }).commit();
    r.write({ 'src/core/one.ts': 'export const one = 1;\n' });
    const { out, leaked } = checkWithIsolatedTmp(r.ctx('main'), ['src/core/one.ts', '--bad/x.ts']);
    expect(leaked).toEqual([]);
    expect(out).toEqual({
      violations: [],
      metrics: [{ name: 'duplication', value: 'error', detail: "error: unknown option '--bad/x.ts'" }],
    });
  });
});
