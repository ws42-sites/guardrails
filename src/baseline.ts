import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import type { Violation } from './types.js';

export interface BaselineEntry { rule: string; file: string; fingerprint: string; message?: string }
export interface Baseline { version: 1; entries: BaselineEntry[] }

export const key = (e: { rule: string; fingerprint: string }): string => `${e.rule}:${e.fingerprint}`;

export function readBaseline(cwd: string, file: string): Baseline {
  const p = join(cwd, file);
  if (!existsSync(p)) return { version: 1, entries: [] };
  const b = JSON.parse(readFileSync(p, 'utf8')) as Baseline;
  return { version: 1, entries: b.entries ?? [] };
}

/**
 * Ratchet: the new baseline is current violations ∩ old baseline, so fixed items drop out
 * and nothing new gets in. `init: true` (no baseline yet, or --force) records everything current.
 */
export function ratchet(old: Baseline, current: Violation[], init: boolean, rules: Set<string>): { baseline: Baseline; removed: number; added: number } {
  const oldKeys = new Set(old.entries.map(key));
  const pick = current.filter((v) => rules.has(v.rule) && (init || oldKeys.has(key(v))));
  const seen = new Set<string>();
  const entries: BaselineEntry[] = [];
  for (const v of pick) {
    const k = key(v);
    if (seen.has(k)) continue;
    seen.add(k);
    entries.push({ rule: v.rule, file: v.file, fingerprint: v.fingerprint, message: v.message.slice(0, 160) });
  }
  entries.sort((a, b) => (a.rule + a.file + a.fingerprint).localeCompare(b.rule + b.file + b.fingerprint));
  const removed = old.entries.filter((e) => !seen.has(key(e))).length;
  const added = entries.filter((e) => !oldKeys.has(key(e))).length;
  return { baseline: { version: 1, entries }, removed, added };
}

export function writeBaseline(cwd: string, file: string, b: Baseline): void {
  const p = join(cwd, file);
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, `${JSON.stringify(b, null, 2)}\n`);
}
