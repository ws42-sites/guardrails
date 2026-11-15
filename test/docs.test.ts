import { symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { checkDocs } from '../src/checks/docs.js';
import { repo, BASE_CONFIG } from './helpers/repo.js';

const long = (n: number) => Array.from({ length: n }, (_, i) => `line ${i}`).join('\n');

describe('DOC-01 only allowlisted docs may exist', () => {
  it('flags docs outside the allowlist, including disguised ones and .txt/.rst', () => {
    const r = repo({ 'README.md': '# r', 'docs/plan.md': 'x', 'notes.txt': 'x', 'guide.rst': 'x', 'NOTES': 'x', 'src/feature-plan.ts': 'x', 'public/robots.txt': 'x' });
    const v = checkDocs(r.ctx()).violations;
    expect(v.filter((x) => x.rule === 'DOC-1').map((x) => x.file).sort()).toEqual(['NOTES', 'docs/plan.md', 'guide.rst', 'notes.txt', 'src/feature-plan.ts']);
  });

  it('requires configured links', () => {
    const r = repo({ 'AGENTS.md': 'see docs/ARCHITECTURE.md' }, { ...BASE_CONFIG, docs: { requiredLinks: { 'AGENTS.md': ['docs/ARCHITECTURE.md', 'docs/TESTING.md'] } } });
    expect(checkDocs(r.ctx()).violations.map((x) => x.message)).toEqual(['must link to docs/TESTING.md.']);
  });
});

describe('DOC-02 agent files point at AGENTS.md', () => {
  it.each([
    ['See AGENTS.md.', 0],
    ['# Gemini\nDo things\nSee AGENTS.md', 1],
    ['Follow the rules', 1],
  ])('GEMINI.md %j -> %i violations', (body, n) => {
    const r = repo({ 'GEMINI.md': body });
    expect(checkDocs(r.ctx()).violations.filter((x) => x.rule === 'DOC-2')).toHaveLength(n);
  });

  it('accepts a symlink and requires AGENTS.md to exist', () => {
    const r = repo();
    symlinkSync('AGENTS.md', join(r.dir, 'CLAUDE.md'));
    expect(checkDocs(r.ctx()).violations).toEqual([]);
    r.remove('AGENTS.md', 'CLAUDE.md');
    expect(checkDocs(r.ctx()).violations.map((x) => [x.rule, x.file])).toEqual([['DOC-2', 'AGENTS.md']]);
  });
});

describe('DOC-03 allowlisted docs have a line cap', () => {
  it('uses per-file caps, then the default', () => {
    const r = repo({ 'AGENTS.md': long(151), 'README.md': long(250), 'docs/ARCHITECTURE.md': long(251) });
    expect(checkDocs(r.ctx()).violations.map((x) => [x.rule, x.file])).toEqual([['DOC-3', 'AGENTS.md'], ['DOC-3', 'docs/ARCHITECTURE.md']]);
  });
});
