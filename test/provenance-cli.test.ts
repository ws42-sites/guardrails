import { execFileSync } from 'node:child_process';
import { expect, it } from 'vitest';
import { record, verify } from '../src/provenance.js';
import { repo } from './helpers/repo.js';
import { config, instant } from './helpers/provenance.js';

it('PROV-01 reads the actual Git checkout synchronously', () => {
  const r = repo({ 'report.json': JSON.stringify({ ok: true, findings: [] }) }).commit();
  const value = record({ cwd: r.dir, report: 'report.json', provenance: config, clock: () => new Date(instant) });
  const sha = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: r.dir, encoding: 'utf8' }).trim();
  expect(value.testedSha).toBe(sha);
  expect(value.parents).toEqual([sha]);
  expect(verify(value, sha)).toEqual({ ok: true, violations: [] });
});

