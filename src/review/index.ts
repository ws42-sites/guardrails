import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { GuardrailsConfig } from '../config.js';
import { git, isTestFile, matchesAny, violation } from '../util.js';
import { isProtected } from '../hook.js';
import { shardFiles } from '../shard.js';
import { createReviewer, reviewerError, type Reviewer, type ReviewInput, type Verdict } from './provider.js';

export interface ReviewOptions { base?: string | null; report?: string; testOutput?: unknown; shard?: string }
export type ReviewResult = Verdict | { verdict: 'skipped'; findings: []; reason: string };

function changes(cwd: string, config: GuardrailsConfig, opts: ReviewOptions) {
  const base = git(cwd, ['merge-base', opts.base ?? `origin/${config.baseBranch}`, 'HEAD']).trim();
  const range = `${base}..HEAD`;
  const files = git(cwd, ['diff', '--no-ext-diff', '--name-only', '-z', '--no-renames', range]).split('\0').filter(Boolean).sort();
  return { base, range, files };
}

export function buildInput(cwd: string, config: GuardrailsConfig, opts: ReviewOptions = {}): ReviewInput {
  const change = changes(cwd, config, opts);
  let files = change.files;
  if (opts.shard !== undefined) {
    if (!/^[1-9]\d*\/[1-9]\d*$/.test(opts.shard)) throw new Error('invalid shard');
    const [index, total] = opts.shard.split('/').map(Number);
    files = shardFiles(cwd, config, files, index, total, { globs: ['**/*'] });
  }
  const text = files.length ? git(cwd, ['diff', '--no-color', '--no-ext-diff', '--no-renames', change.range, '--', ...files]) : '';
  const bytes = Buffer.from(text);
  let end = Math.min(bytes.length, config.review.maxDiffBytes);
  while (end > 0 && end < bytes.length && (bytes[end] & 0xc0) === 0x80) end--;
  return { diff: { text: bytes.subarray(0, end).toString(), truncated: bytes.length > config.review.maxDiffBytes }, testOutput: opts.report ? JSON.parse(readFileSync(resolve(cwd, opts.report), 'utf8')) : opts.testOutput ?? [], files };
}

export async function review(cwd: string, config: GuardrailsConfig, opts: ReviewOptions = {}, reviewer: Reviewer = createReviewer(config.review)): Promise<ReviewResult> {
  if (!config.review.enabled) return { verdict: 'skipped', findings: [], reason: 'review disabled' };
  try {
    if (config.review.onlyWhenRisky) {
      const change = changes(cwd, config, opts);
      const risky = change.files.some((file) => {
        if (matchesAny(file, ['guardrails.config.json', 'guardrails.md', '.guardrails/**', ...config.review.paths]) || isProtected(cwd, config, file)) return true;
        if (!config.lock.specTests || !isTestFile(config, file)) return false;
        try { return new RegExp(config.specs.idPattern.replace(/^\^|\$$/g, '')).test(git(cwd, ['show', `${change.base}:${file}`])); } catch { return false; }
      });
      if (!risky) return { verdict: 'skipped', findings: [], reason: 'no risky files changed' };
    }
    const input = buildInput(cwd, config, opts);
    if (!input.files.length) return { verdict: 'skipped', findings: [], reason: 'no changed files in shard' };
    return await reviewer.review(input);
  } catch { return reviewerError('unable to build input or run reviewer'); }
}

export function reviewViolations(result: ReviewResult) {
  return result.verdict === 'fail' ? result.findings.map((f) => violation('REVIEW-1', f.file, f.message, f.message, f.line)) : [];
}
