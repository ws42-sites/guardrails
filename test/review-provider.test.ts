import { existsSync, readdirSync } from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';
import { createReviewer, INSTRUCTIONS, reviewerError, type Verdict } from '../src/review/provider.js';
import { config, fakeSpawn, input } from './helpers/review.js';

afterEach(() => { vi.useRealTimers(); vi.unstubAllEnvs(); });

it.each([
  ['+++ b/a.ts\r\n@@ -2,2 +4,3 @@\r\n old\r\n-bad\r\n+bad\r\n+ok', [{ file: 'a.ts', line: 5, message: 'Added line matches reviewer failOn: bad' }]],
  ['diff --git a/a b/a\n+++ "b/a b.ts"\n@@ -0,0 +1 @@\n+bad\n\\ No newline at end of file', [{ file: 'a b.ts', line: 1, message: 'Added line matches reviewer failOn: bad' }]],
  ['+++ b/a.ts\n@@ -1 +1 @@\n-bad\n+good\ndiff --git a/b b/b\n+bad', []],
  ['+bad\n+++ b/a.ts\n+bad', []],
])('REVIEW-01 stub checks only added lines with new-side locations: %s', async (text, findings) => {
  const provider = createReviewer(config({ stub: { failOn: ['bad', 'worse'] } }).review);
  expect(provider.name).toBe('stub');
  expect(await provider.review({ ...input, diff: { text, truncated: false } })).toEqual({ verdict: findings.length ? 'fail' : 'pass', findings });
});

it('REVIEW-01 invalid stub patterns fail closed', async () => {
  expect(await createReviewer(config({ stub: { failOn: ['['] } }).review).review(input)).toEqual(reviewerError('invalid stub pattern or diff'));
});

it.each<[number | null, string, Verdict]>([
  [0, '{"verdict":"pass","findings":[]}', { verdict: 'pass', findings: [] }],
  [0, '{"verdict":"fail","findings":[{"file":"a","line":2,"message":"bug"}]}', { verdict: 'fail', findings: [{ file: 'a', line: 2, message: 'bug' }] }],
  [1, '{"verdict":"pass","findings":[]}', reviewerError('non-zero exit')],
  [null, '{}', reviewerError('non-zero exit')],
  [0, '{', reviewerError('invalid JSON')],
  ...['{}', '{"verdict":"maybe","findings":[]}', '{"verdict":"fail","findings":[]}', '{"verdict":"pass","findings":[{"file":"a","message":"bug"}]}', '{"verdict":"fail","findings":[{"file":"","message":"bug"}]}', '{"verdict":"fail","findings":[{"file":"a","line":0,"message":"bug"}]}', '{"verdict":"fail","findings":[{"file":"a","line":1.5,"message":"bug"}]}', '{"verdict":"fail","findings":[{"file":"a","message":""}]}', '{"verdict":"pass","findings":[],"extra":true}', '{"verdict":"fail","findings":[{"file":"a","message":"bug","extra":true}]}'].map((json) => [0, json, reviewerError('invalid verdict')] as [number, string, Verdict]),
])('REVIEW-02 command validates verdicts (%s %s)', async (code, output, expected) => {
  const fake = fakeSpawn((child) => { child.stdout.write(String(output)); child.emit('close', code); });
  expect(await createReviewer(config({ provider: 'command', command: 'review-code' }).review, fake.spawn).review(input)).toEqual(expected);
});

it('REVIEW-02 command isolates cwd and environment and sends only approved input', async () => {
  vi.stubEnv('REVIEW_SECRET', 'secret');
  vi.stubEnv('HOME', 'review-home');
  const fake = fakeSpawn((child) => { child.stdout.write('{"verdict":"pass",'); child.stdout.write('"findings":[]}'); child.emit('close', 0); });
  let directory = '';
  const provider = createReviewer(config({ provider: 'command', command: 'review-code' }).review, (command, options) => {
    directory = options.cwd;
    expect(command).toBe('review-code');
    expect(readdirSync(directory)).toEqual([]);
    expect(options.shell).toBe(true);
    expect(options.stdio).toBe('pipe');
    expect(options.env).toEqual(Object.fromEntries(['PATH', 'HOME', 'USERPROFILE', 'SystemRoot', 'TEMP', 'TMP'].filter((key) => process.env[key] !== undefined).map((key) => [key, process.env[key]])));
    return fake.spawn(command, options);
  });
  expect(provider.name).toBe('command');
  expect(await provider.review(input)).toEqual({ verdict: 'pass', findings: [] });
  expect(fake.sent()).toEqual({ ...input, instructions: INSTRUCTIONS });
  expect(INSTRUCTIONS).toBe('Review only this code and test output. Report real defects with file and line, and return JSON {"verdict":"pass"|"fail","findings":[{"file":"path","line":1,"message":"defect"}]}.');
  expect(existsSync(directory)).toBe(false);
});

it.each(['throw', 'error', 'stdin'])('REVIEW-02 command handles %s errors', async (failure) => {
  const fake = fakeSpawn((child) => { if (failure === 'stdin') child.stdin.emit('error', new Error('private')); else child.emit('error', new Error('private')); });
  const result = await createReviewer(config({ provider: 'command', command: 'x' }).review, failure === 'throw' ? () => { throw new Error('private'); } : fake.spawn).review(input);
  expect(result).toEqual(reviewerError(failure === 'stdin' ? 'stdin failed' : 'spawn failed'));
});

it('REVIEW-02 timeout kills the command and clears completed timers', async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-01-01T00:00:00Z'));
  const fake = fakeSpawn(() => {});
  const result = createReviewer(config({ provider: 'command', command: 'x', timeoutSeconds: 2 }).review, fake.spawn).review(input);
  await vi.advanceTimersByTimeAsync(1999);
  expect(vi.mocked(fake.child.kill).mock.calls).toEqual([]);
  await vi.advanceTimersByTimeAsync(1);
  expect(await result).toEqual(reviewerError('timeout'));
  expect(vi.mocked(fake.child.kill).mock.calls).toEqual([[]]);
  expect(vi.getTimerCount()).toBe(0);
});
