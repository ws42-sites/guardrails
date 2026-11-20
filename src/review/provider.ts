import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { z } from 'zod';
import type { GuardrailsConfig } from '../config.js';

export interface ReviewInput { diff: { text: string; truncated: boolean }; testOutput: unknown; files: string[] }
const findingSchema = z.object({ file: z.string().min(1), line: z.number().int().positive().optional(), message: z.string().min(1) }).strict();
const verdictSchema = z.object({ verdict: z.enum(['pass', 'fail']), findings: z.array(findingSchema) }).strict()
  .refine((v) => v.verdict === 'fail' ? v.findings.length > 0 : v.findings.length === 0);
export type Verdict = z.infer<typeof verdictSchema>;
export interface Reviewer { name: string; review(input: ReviewInput): Promise<Verdict> }
export const INSTRUCTIONS = 'Review only this code and test output. Report real defects with file and line, and return JSON {"verdict":"pass"|"fail","findings":[{"file":"path","line":1,"message":"defect"}]}.';
export const reviewerError = (message: string): Verdict => ({ verdict: 'fail', findings: [{ file: 'guardrails.config.json', message: `reviewer error: ${message}` }] });
export type Spawner = (command: string, options: { cwd: string; env: NodeJS.ProcessEnv; shell: true; stdio: 'pipe' }) => ChildProcessWithoutNullStreams;

export function createReviewer(config: GuardrailsConfig['review'], spawner: Spawner = spawn): Reviewer {
  return {
    name: config.provider,
    async review(input) {
      if (config.provider === 'stub') {
        try {
          const patterns = config.stub.failOn.map((pattern) => new RegExp(pattern));
          const findings: Verdict['findings'] = [];
          let file = '', line = 0;
          for (const text of input.diff.text.split(/\r?\n/)) {
            if (text.startsWith('diff --git ')) { file = ''; line = 0; }
            else if (line === 0 && text.startsWith('+++ b/')) file = text.slice(6);
            else if (line === 0 && text.startsWith('+++ "b/')) file = (JSON.parse(text.slice(4)) as string).slice(2);
            else if (text.startsWith('@@ ')) line = Number(/\+(\d+)/.exec(text)?.[1]);
            else if (file && line > 0) {
              if (text.startsWith('+') && patterns.some((pattern) => pattern.test(text.slice(1)))) findings.push({ file, line, message: `Added line matches reviewer failOn: ${text.slice(1)}` });
              if (text.startsWith('+') || text.startsWith(' ')) line++;
            }
          }
          return { verdict: findings.length ? 'fail' : 'pass', findings };
        } catch { return reviewerError('invalid stub pattern or diff'); }
      }
      const cwd = mkdtempSync(join(tmpdir(), 'guardrails-review-'));
      try {
        const env: NodeJS.ProcessEnv = {};
        for (const key of ['PATH', 'HOME', 'USERPROFILE', 'SystemRoot', 'TEMP', 'TMP']) if (process.env[key] !== undefined) env[key] = process.env[key];
        return await new Promise<Verdict>((resolve) => {
          const child = spawner(config.command ?? '', { cwd, env, shell: true, stdio: 'pipe' });
          let output = '';
          const finish = (result: Verdict) => { clearTimeout(timer); resolve(result); };
          const timer = setTimeout(() => { child.kill(); finish(reviewerError('timeout')); }, config.timeoutSeconds * 1000);
          child.stdout.on('data', (chunk: Buffer) => { output += chunk.toString(); });
          child.stderr.resume();
          child.on('error', () => finish(reviewerError('spawn failed')));
          child.stdin.on('error', () => finish(reviewerError('stdin failed')));
          child.on('close', (code) => {
            if (code !== 0) { finish(reviewerError('non-zero exit')); return; }
            try {
              const parsed = verdictSchema.safeParse(JSON.parse(output));
              finish(parsed.success ? parsed.data : reviewerError('invalid verdict'));
            } catch { finish(reviewerError('invalid JSON')); }
          });
          child.stdin.end(JSON.stringify({ ...input, instructions: INSTRUCTIONS }));
        });
      } catch { return reviewerError('spawn failed'); }
      finally { rmSync(cwd, { recursive: true, force: true }); }
    },
  };
}
