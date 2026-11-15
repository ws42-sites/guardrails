import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { run } from '../src/runner.js';
import { ratchet, readBaseline, writeBaseline } from '../src/baseline.js';
import { toMarkdown, toText } from '../src/report.js';
import { RULES } from '../src/rules.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const cheat = 'export const quiet = !!process.env.VITEST;\n';
const ok = `it('CORE-01 adds', () => { expect(1 + 1).toBe(2) });\nit('LIB-01 formats', () => { expect('a').toBe('a') });\n`;

describe('MODE-01 modes, overrides and the baseline decide what blocks', () => {
  it('passes a clean repo and fails on a blocking finding', async () => {
    const r = repo({ 'src/core/a.test.ts': ok });
    expect((await run({ cwd: r.dir, knip: false })).counts).toEqual({ block: 0, warn: 0, known: 0 });
    r.write({ 'src/x.ts': cheat, 'docs/plan.md': 'x' });
    const rep = await run({ cwd: r.dir, knip: false });
    expect([rep.ok, rep.counts, rep.findings.map((f) => `${f.rule}:${f.mode}`)]).toEqual([false, { block: 1, warn: 1, known: 0 }, ['DOC-1:warn', 'ARCH-7:block']]);
  });

  it('applies rule overrides and reports an invalid config as CFG-1', async () => {
    const r = repo({ 'src/core/a.test.ts': ok, 'src/x.ts': cheat }, { ...BASE_CONFIG, rules: { 'ARCH-7': 'warn' } });
    expect((await run({ cwd: r.dir, knip: false })).counts).toEqual({ block: 0, warn: 1, known: 0 });
    r.write({ 'guardrails.config.json': '{"version":1,"oops":true}' });
    const rep = await run({ cwd: r.dir, knip: false });
    expect([rep.ok, rep.findings[0].rule]).toEqual([false, 'CFG-1']);
  });
});

describe('BASE-01 the baseline records known violations and only shrinks', () => {
  it('known violations stop blocking; fixed ones drop out on ratchet; new ones never get in', async () => {
    const r = repo({ 'src/core/a.test.ts': ok, 'src/a.ts': cheat, 'src/b.ts': cheat });
    const first = await run({ cwd: r.dir, knip: false });
    const init = ratchet(readBaseline(r.dir, '.guardrails/baseline.json'), first.findings, true, new Set(['ARCH-7']));
    writeBaseline(r.dir, '.guardrails/baseline.json', init.baseline);
    expect((await run({ cwd: r.dir, knip: false })).counts).toEqual({ block: 0, warn: 0, known: 2 });

    r.remove('src/a.ts').write({ 'src/c.ts': cheat });
    const now = await run({ cwd: r.dir, knip: false });
    expect([now.counts, now.staleBaseline]).toEqual([{ block: 1, warn: 0, known: 1 }, 1]);
    const next = ratchet(readBaseline(r.dir, '.guardrails/baseline.json'), now.findings, false, new Set(['ARCH-7']));
    expect([next.baseline.entries.map((e) => e.file), next.removed, next.added]).toEqual([['src/b.ts'], 1, 0]);
  });
});

describe('REPORT-01 reports are readable for people, agents and PRs', () => {
  it('renders text with the fix hint and a markdown table', async () => {
    const r = repo({ 'src/core/a.test.ts': ok, 'src/x.ts': cheat });
    const rep = await run({ cwd: r.dir, knip: false });
    expect(toText(rep).split('\n')).toEqual([
      '✖ ARCH-7 Test-gaming signatures in product code (1)',
      '    src/x.ts:1  product code checks whether it runs under a test runner. Behave the same in tests and production.',
      '    fix: Remove the special case and implement the behaviour. If the test looks wrong, call guardrails_flag instead.',
      '• tests: 2',
      '• specs: 2 (2 with tests)',
      'FAIL: 1 blocking, 0 warnings, 0 known',
    ]);
    expect(toMarkdown(rep)).toContain('| ARCH-7 | block | `src/x.ts:1` | product code checks');
  });
});

describe('RULES-01 the rules catalogue and RULES.md agree', () => {
  it('documents every rule ID and no others', () => {
    const md = readFileSync(join(process.cwd(), 'RULES.md'), 'utf8');
    const documented = [...md.matchAll(/^\| (\w+-\d+W?) \|/gm)].map((m) => m[1]);
    expect(documented).toEqual(RULES.map((r) => r.id));
  });
});
