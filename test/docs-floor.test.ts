import { expect, it } from 'vitest';
import { checkDocs } from '../src/checks/docs.js';
import { repo, BASE_CONFIG } from './helpers/repo.js';

const details = (r: ReturnType<typeof repo>) => checkDocs(r.ctx()).violations.map(({ rule, file, line, message }) => ({ rule, file, line, message }));

it.each([
  ['guide.MD', 'not in docs.allow. Move the content into AGENTS.md, another allowlisted doc or a check, then delete it.'],
  ['notes.txt', 'not in docs.allow. Move the content into AGENTS.md, another allowlisted doc or a check, then delete it.'],
  ['NOTES', 'looks like a doc in disguise (notes/todo/plan file). Put it in an allowlisted doc or delete it.'],
])('DOC-01 reports the exact reason for %s', (file, message) => {
  expect(details(repo({ [file]: 'text' }))).toEqual([{ rule: 'DOC-1', file, line: undefined, message }]);
});

it.each([true, false])('DOC-01 only exempts the configured runbook when enabled=%s', (enabled) => {
  const r = repo({ 'OPERATIONS.md': 'a\nb', 'other.md': 'x' }, { ...BASE_CONFIG, runbook: { enabled, file: 'OPERATIONS.md' } });
  expect(details(r)).toEqual((enabled ? ['other.md'] : ['OPERATIONS.md', 'other.md']).map((file) => ({ rule: 'DOC-1', file, line: undefined, message: 'not in docs.allow. Move the content into AGENTS.md, another allowlisted doc or a check, then delete it.' })));
});

it('DOC-01 checks exact required links and ignores absent documents', () => {
  const r = repo({ 'README.md': 'TESTING.md' }, { ...BASE_CONFIG, docs: { requiredLinks: { 'README.md': ['TESTING.md', 'RULES.md'], 'absent.md': ['x'] } } });
  expect(details(r)).toEqual([{ rule: 'DOC-1', file: 'README.md', line: undefined, message: 'must link to RULES.md.' }]);
});

it.each(['\r\n \t\r\nSee AGENTS.md.\r\n \t', 'See AGENTS.md.\n\n', 'See AGENTS.md.\n  '])('DOC-02 accepts blank space around a pointer %j', (body) => {
  expect(details(repo({ 'GEMINI.md': body }))).toEqual([]);
});

it.each(['See AGENTS.md.\nMore rules', 'See another file.', ''])('DOC-02 diagnoses invalid pointer %j', (body) => {
  expect(details(repo({ 'GEMINI.md': body }))).toEqual([{ rule: 'DOC-2', file: 'GEMINI.md', line: undefined, message: 'must be a one-line pointer to AGENTS.md (e.g. "See AGENTS.md."), or a symlink to it. Move its content into AGENTS.md.' }]);
});

it('DOC-02 diagnoses a missing canonical file', () => {
  expect(details(repo().remove('AGENTS.md'))).toEqual([{ rule: 'DOC-2', file: 'AGENTS.md', line: undefined, message: 'missing AGENTS.md, the canonical agent instruction file. Run "guardrails init".' }]);
});

it.each([
  [{ 'docs/README.md': 1, 'README.md': 3, default: 4 }, 2, ['DOC-3']],
  [{ 'README.md': 1, default: 4 }, 2, ['DOC-3']],
  [{ default: 1 }, 2, ['DOC-3']],
  [{ default: 2 }, 2, []],
])('DOC-03 respects full path, basename, default and boundary caps %j', (maxLines, count, rules) => {
  const r = repo({ 'docs/README.md': Array.from({ length: count }, () => 'line').join('\r\n') }, { ...BASE_CONFIG, docs: { allow: ['docs/README.md'], maxLines } });
  expect(details(r)).toEqual(rules.map((rule) => ({ rule, file: 'docs/README.md', line: undefined, message: '2 lines (cap 1). Move rules into checks and trim it.' })));
});

