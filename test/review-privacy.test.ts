import { expect, it } from 'vitest';
import { createReviewer } from '../src/review/provider.js';
import { buildInput } from '../src/review/index.js';
import { git } from '../src/util.js';
import { repo } from './helpers/repo.js';
import { config } from './helpers/review.js';

it('REVIEW-02 input excludes author metadata and working changes and reads JSON reports', async () => {
  const r = repo({ 'é[1].ts': 'old\n' }).commit('PRIVATE_MARKER');
  git(r.dir, ['checkout', '-b', 'PRIVATE_MARKER']);
  r.write({ 'é[1].ts': 'new\n', 'report.json': '{"findings":[{"rule":"TEST-1"}]}' }).commit('PRIVATE_MARKER');
  r.write({ 'é[1].ts': 'uncommitted\n' });
  const result = buildInput(r.dir, config(), { base: 'main', report: 'report.json' });
  expect(Object.keys(result)).toEqual(['diff', 'testOutput', 'files']);
  expect(result.files).toEqual(['report.json', 'é[1].ts']);
  expect(result.testOutput).toEqual({ findings: [{ rule: 'TEST-1' }] });
  expect(result.diff).toEqual({ text: git(r.dir, ['--literal-pathspecs', '-c', 'core.quotePath=false', 'diff', '--no-color', '--no-ext-diff', '--no-renames', 'main..HEAD', '--', 'report.json', 'é[1].ts']), truncated: false });
  expect(await createReviewer(config({ stub: { failOn: ['^new$'] } }).review).review(result)).toEqual({ verdict: 'fail', findings: [{ file: 'é[1].ts', line: 1, message: 'Added line matches reviewer failOn: new' }] });
  expect(JSON.stringify(result).includes('PRIVATE_MARKER')).toBe(false);
  expect(result.diff.text.includes('uncommitted')).toBe(false);
});

