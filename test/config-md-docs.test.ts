import { expect, it } from 'vitest';
import { checkDocs } from '../src/checks/docs.js';
import { CONFIG_MD } from '../src/config.js';
import { repo } from './helpers/repo.js';

const rows: { files: Record<string, string>; expected: { rule: string; file: string }[] }[] = [
  { files: { 'guardrails.md': '---\nreview.enabled: false\n---\n# Guardrails\n' }, expected: [] },
  { files: { 'docs/guardrails.md': '# Notes\n' }, expected: [{ rule: 'DOC-1', file: 'docs/guardrails.md' }] },
];

it.each(rows)('CFGMD-02 DOC-1 treats only the root guardrails.md as config: %#', ({ files, expected }) => {
  const r = repo({ 'AGENTS.md': '# Agents\n', ...files }).commit();
  const found = checkDocs(r.ctx()).violations.filter((v) => v.rule === 'DOC-1').map((v) => ({ rule: v.rule, file: v.file }));
  expect({ name: CONFIG_MD, found }).toEqual({ name: 'guardrails.md', found: expected });
});
