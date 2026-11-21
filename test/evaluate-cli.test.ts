import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { expect, it } from 'vitest';
import { evaluate } from '../src/evaluate.js';
import { RULES } from '../src/rules.js';
import { repo } from './helpers/repo.js';
const cli = resolve('dist/cli.js');
it('EVAL-01 writes evaluation reports without changing config', () => {
  const r = repo();
  const initial = readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8');
  const output = execFileSync(process.execPath, [cli, 'evaluate', '--cwd', r.dir, '--since', '1970-01-01', '--format', 'json', '--out', 'report.json', '--cache', 'cache'], { encoding: 'utf8' });
  expect(output).toBe('');
  expect(JSON.parse(readFileSync(join(r.dir, 'report.json'), 'utf8'))).toEqual(evaluate([], RULES, r.ctx().config, { now: new Date('2026-01-08T00:00:00Z'), since: '1970-01-01' }));
  expect(readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8')).toBe(initial);
});
