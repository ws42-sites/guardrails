import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { buildInput, review, reviewViolations } from '../src/review/index.js';
import { reviewerError } from '../src/review/provider.js';
import { violation } from '../src/util.js';
import { run } from '../src/runner.js';
import { config, tree } from './helpers/review.js';

it('REVIEW-02 truncation uses UTF8 bytes without splitting characters and default base', () => {
  const r = tree({ 'a.ts': '' }, 'aé🍁\n');
  for (const [limit, text] of [[1, 'a'], [2, 'a'], [3, 'aé'], [6, 'aé'], [7, 'aé🍁'], [8, 'aé🍁\n'], [9, 'aé🍁\n']] as const) {
    expect(buildInput(r.dir, config({ maxDiffBytes: limit })).diff).toEqual({ text, truncated: limit < 8 });
  }
  expect(r.calls[0]).toEqual(['merge-base', 'origin/main', 'HEAD']);
  expect(r.calls[1]).toEqual(['diff', '--no-ext-diff', '--name-only', '-z', '--no-renames', 'abc..HEAD']);
  expect(r.calls[2]).toEqual(['--literal-pathspecs', '-c', 'core.quotePath=false', 'diff', '--no-color', '--no-ext-diff', '--no-renames', 'abc..HEAD', '--', 'a.ts']);
  expect(buildInput(r.dir, config()).testOutput).toEqual([]);
});

it('REVIEW-01 shards are stable and size balanced and scope Git to selected files', () => {
  const r = tree({ 'c.ts': 'c\n', 'b.ts': 'b\nb\n', 'a.ts': 'a\na\na\na\n' });
  expect(buildInput(r.dir, config(), { base: 'base', shard: '1/2' }).files).toEqual(['a.ts']);
  expect(r.calls[0]).toEqual(['merge-base', 'base', 'HEAD']);
  expect(r.calls[2]).toEqual(['--literal-pathspecs', '-c', 'core.quotePath=false', 'diff', '--no-color', '--no-ext-diff', '--no-renames', 'abc..HEAD', '--', 'a.ts']);
  const two = buildInput(r.dir, config(), { base: 'base', shard: '2/2', testOutput: { findings: [] } });
  expect(two.files).toEqual(['b.ts', 'c.ts']);
  expect(two.testOutput).toEqual({ findings: [] });
  expect(buildInput(r.dir, config(), { base: 'base' }).files).toEqual(['a.ts', 'b.ts', 'c.ts']);
  expect(buildInput(r.dir, config(), { base: 'base', shard: '10/10' }).files).toEqual([]);
  expect(buildInput(r.dir, config(), { base: 'base', shard: '4/4' })).toEqual({ diff: { text: '', truncated: false }, files: [], testOutput: [] });
});

it.each([
  ['plain.ts', 'safe', false], ['auth/a.ts', 'safe', true], ['guardrails.md', 'safe', true],
  ['guardrails.config.json', '{}', true], ['.guardrails/data', 'safe', true], ['test/a.test.ts', "it('CORE-01 does work')", true],
  ['test/a.test.ts', 'untagged', false], ['custom/a.ts', 'safe', true], ['.github/workflows/ci.yml', 'safe', false],
])('REVIEW-01 risk trigger checks %s', async (file, body, risky) => {
  const r = tree({ 'safe.txt': 'safe', [file]: body });
  const cfg = config({ onlyWhenRisky: true, paths: ['auth/**', 'custom/**'] });
  for (const paths of [cfg.lock.paths, []]) {
    cfg.lock.paths = paths;
    expect(await review(r.dir, cfg, { base: 'base' })).toEqual(risky ? { verdict: 'pass', findings: [] } : { verdict: 'skipped', findings: [], reason: 'no risky files changed' });
  }
});

it.each([[true, 'CORE-01'], [false, 'CORE-01'], [true, 'A^-01'], [true, 'A$-01']] as const)('REVIEW-01 deleted or stripped spec tests trigger review: deleted %s %s', async (remove, id) => {
  const r = tree(remove ? {} : { 'a.test.ts': 'plain' }, '', { 'a.test.ts': `it('${id} behaviour')` });
  const cfg = config({ onlyWhenRisky: true });
  cfg.specs.idPattern = '^[A-Z^$]+-[0-9]{2}$';
  expect(await review(r.dir, cfg, { base: 'base' })).toEqual({ verdict: 'pass', findings: [] });
  cfg.lock.specTests = false;
  expect(await review(r.dir, cfg, { base: 'base' })).toEqual({ verdict: 'skipped', findings: [], reason: 'no risky files changed' });
});

it.each(['0/2', '1/0', '3/2', '01/2', '1/02', '1.5/2', '1/2/3', '', 'bad-base', 'missing-report', 'throw-provider'])('REVIEW-02 input failures fail closed: %s', async (failure) => {
  const r = tree({ 'a.ts': 'new' });
  const opts = failure === 'bad-base' ? { base: 'missing' } : failure === 'missing-report' ? { base: 'base', report: 'missing' } : failure === 'throw-provider' ? { base: 'base' } : { base: 'base', shard: failure };
  expect(await review(r.dir, config(), opts, { name: 'test', async review() { if (failure === 'throw-provider') throw new Error('private'); return { verdict: 'pass', findings: [] }; } })).toEqual(reviewerError('unable to build input or run reviewer'));
  if (['0/2', '1/0', '01/2', '1/02', '1.5/2', '1/2/3', ''].includes(failure)) {
    let message = '';
    try { buildInput(r.dir, config(), opts); } catch (error) { message = (error as Error).message; }
    expect(message).toBe('invalid shard');
  }
});

it('REVIEW-01 runner is opt-in, honours group selection and maps fail findings', async () => {
  const cfg = config({ stub: { failOn: ['bad'] } });
  const r = tree({ 'a.ts': 'bad\n' }, '+++ b/a.ts\n@@ -0,0 +1 @@\n+bad\n');
  const finding = { file: 'a.ts', line: 1, message: 'Added line matches reviewer failOn: bad' };
  expect(reviewViolations({ verdict: 'fail', findings: [finding] })).toEqual([violation('REVIEW-1', 'a.ts', finding.message, finding.message, 1)]);
  expect(reviewViolations({ verdict: 'pass', findings: [] })).toEqual([]);
  expect(reviewViolations({ verdict: 'pass', findings: [finding] })).toEqual([]);
  for (const [enabled, requested, only, blocks] of [[true, true, ['review'], 1], [true, false, ['review'], 0], [false, true, ['review'], 0], [true, true, ['flags'], 0]] as const) {
    writeFileSync(join(r.dir, 'guardrails.config.json'), JSON.stringify({ ...cfg, review: { ...cfg.review, enabled } }));
    const result = await run({ cwd: r.dir, base: 'base', review: requested, only: [...only], knip: false });
    expect(result.counts.block).toBe(blocks);
    expect(result.ok).toBe(blocks === 0);
    expect(result.findings.map((f) => f.rule)).toEqual(blocks ? ['REVIEW-1'] : []);
  }
});

it('REVIEW-01 disabled and empty reviews skip without invoking a provider', async () => {
  const r = tree({});
  expect(await review('missing', config({ enabled: false }))).toEqual({ verdict: 'skipped', findings: [], reason: 'review disabled' });
  expect(await review(r.dir, config(), { base: 'HEAD' })).toEqual({ verdict: 'skipped', findings: [], reason: 'no changed files in shard' });
});
