import { expect, it } from 'vitest';
import { buildInput } from '../src/review/index.js';
import { git } from '../src/util.js';
import { repo } from './helpers/repo.js';
import { config } from './helpers/review.js';

it('REVIEW-02 input excludes author metadata and working changes and reads JSON reports', () => {
  const r = repo({ 'a.ts': 'old\n' }).commit('PRIVATE_MARKER');
  git(r.dir, ['checkout', '-b', 'PRIVATE_MARKER']);
  r.write({ 'a.ts': 'new\n', 'report.json': '{"findings":[{"rule":"TEST-1"}]}' }).commit('PRIVATE_MARKER');
  r.write({ 'a.ts': 'uncommitted\n' });
  const result = buildInput(r.dir, config(), { base: 'main', report: 'report.json' });
  expect(Object.keys(result)).toEqual(['diff', 'testOutput', 'files']);
  expect(result.files).toEqual(['a.ts', 'report.json']);
  expect(result.testOutput).toEqual({ findings: [{ rule: 'TEST-1' }] });
  expect(result.diff).toEqual({ text: git(r.dir, ['diff', '--no-color', '--no-ext-diff', '--no-renames', 'main..HEAD', '--', 'a.ts', 'report.json']), truncated: false });
  expect(JSON.stringify(result).includes('PRIVATE_MARKER')).toBe(false);
  expect(result.diff.text.includes('uncommitted')).toBe(false);
});

