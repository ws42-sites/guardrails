import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { run } from '../src/runner.js';
import { ratchet, readBaseline, writeBaseline } from '../src/baseline.js';
import { toMarkdown, toText } from '../src/report.js';
import { RULES } from '../src/rules.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const ok = `it('CORE-01 adds', () => { expect(1 + 1).toBe(2) });\nit('LIB-01 formats', () => { expect('a').toBe('a') });\n`;

describe('MODE-01 modes, overrides and the baseline decide what blocks', () => {
  it('passes a clean repo and fails on a blocking finding', async () => {
    const r = repo({ 'src/core/a.test.ts': ok });
    expect((await run({ cwd: r.dir, knip: false })).counts).toEqual({ block: 0, warn: 0, known: 0 });
    r.write({ 'docs/plan.md': 'x' });
    const rep = await run({ cwd: r.dir, knip: false });
    expect([rep.ok, rep.counts.block, rep.findings[0].rule]).toEqual([false, 1, 'DOC-1']);
  });

  it('applies rule overrides and reports an invalid config as CFG-1', async () => {
    const r = repo({ 'src/core/a.test.ts': ok, 'docs/plan.md': 'x' }, { ...BASE_CONFIG, rules: { 'DOC-1': 'warn' } });
    expect((await run({ cwd: r.dir, knip: false })).counts).toEqual({ block: 0, warn: 1, known: 0 });
    r.write({ 'guardrails.config.json': '{"version":1,"oops":true}' });
    const rep = await run({ cwd: r.dir, knip: false });
    expect([rep.ok, rep.findings[0].rule]).toEqual([false, 'CFG-1']);
  });
});

describe('BASE-01 the baseline records known violations and only shrinks', () => {
  it('known violations stop blocking; fixed ones drop out on ratchet; new ones never get in', async () => {
    const r = repo({ 'src/core/a.test.ts': ok, 'docs/a.md': 'x', 'docs/b.md': 'x' });
    const first = await run({ cwd: r.dir, knip: false });
    const init = ratchet(readBaseline(r.dir, '.guardrails/baseline.json'), first.findings, true, new Set(['DOC-1']));
    writeBaseline(r.dir, '.guardrails/baseline.json', init.baseline);
    expect((await run({ cwd: r.dir, knip: false })).counts).toEqual({ block: 0, warn: 0, known: 2 });

    r.remove('docs/a.md').write({ 'docs/c.md': 'x' });
    const now = await run({ cwd: r.dir, knip: false });
    expect([now.counts, now.staleBaseline]).toEqual([{ block: 1, warn: 0, known: 1 }, 1]);
    const next = ratchet(readBaseline(r.dir, '.guardrails/baseline.json'), now.findings, false, new Set(['DOC-1']));
    expect([next.baseline.entries.map((e) => e.file), next.removed, next.added]).toEqual([['docs/b.md'], 1, 0]);
  });
});

describe('REPORT-01 reports are readable for people, agents and PRs', () => {
  it('renders text with the fix hint and a markdown table', async () => {
    const r = repo({ 'src/core/a.test.ts': ok, 'notes.txt': 'x' });
    const rep = await run({ cwd: r.dir, knip: false });
    expect(toText(rep).split('\n')).toEqual([
      '✖ DOC-1 Doc allowlist (1)',
      '    notes.txt  not in docs.allow. Put the content in AGENTS.md or docs/ARCHITECTURE.md (or a check), then delete it.',
      '    fix: Put the content in an allowlisted doc (or a check), then delete the file.',
      '• tests: 2',
      '• specs: 2 (2 with tests)',
      'FAIL: 1 blocking, 0 warnings, 0 known',
    ]);
    expect(toMarkdown(rep)).toContain('| DOC-1 | block | `notes.txt` | not in docs.allow.');
  });
});

describe('RULES-01 the rules catalogue and RULES.md agree', () => {
  it('documents every rule ID and no others', () => {
    const md = readFileSync(join(process.cwd(), 'RULES.md'), 'utf8');
    const documented = [...md.matchAll(/^\| (\w+-\d+W?) \|/gm)].map((m) => m[1]);
    expect(documented).toEqual(RULES.map((r) => r.id));
  });
});
