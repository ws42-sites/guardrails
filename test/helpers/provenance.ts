import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach } from 'vitest';
import { record } from '../../src/provenance.js';

export const tested = 'a'.repeat(40);
export const head = 'b'.repeat(40);
export const first = 'c'.repeat(40);
export const instant = '2026-01-02T03:04:05.000Z';
export const config = { enabled: false, out: 'proof.json', overrideLabelPrefix: 'guardrails:' };
const dirs: string[] = [];
afterEach(() => { for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true }); });
export function fixture(parents = [tested]) {
  const cwd = mkdtempSync(join(tmpdir(), 'prov-test-'));
  dirs.push(cwd);
  const report = 'report.json';
  const body = JSON.stringify({ ok: false, findings: [{ rule: 'TEST-1', mode: 'block' }, { rule: 'TEST-1', mode: 'block' }, { rule: 'TEST-1', mode: 'warn' }, { rule: 'ARCH-1', mode: 'warn' }] });
  writeFileSync(join(cwd, report), body);
  const calls: [string[], string][] = [];
  const opts = { cwd, report, provenance: config, clock: () => new Date(instant), git: (args: string[], dir: string) => {
    calls.push([args, dir]);
    return args[0] === 'rev-parse' ? tested + '\r\n' : parents.join(' ') + '\r\n';
  } };
  const value = record(opts);
  return { cwd, opts, value, calls, body, saved: () => JSON.parse(readFileSync(join(cwd, config.out), 'utf8')) };
}
