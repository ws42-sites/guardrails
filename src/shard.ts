import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { GuardrailsConfig } from './config.js';
import { isE2eFile, isTestFile, lineCount, matchesAny } from './util.js';

/**
 * Split the test files into `total` shards balanced by line count (longest first onto the lightest
 * shard), deterministic for a given tree. Returns shard `index` (1-based).
 */
export function shardFiles(cwd: string, cfg: GuardrailsConfig, files: string[], index: number, total: number, opts: { e2e?: boolean; globs?: string[] } = {}): string[] {
  if (!(total >= 1 && index >= 1 && index <= total)) throw new Error(`shard ${index}/${total} is out of range`);
  const pick = files.filter((f) => (opts.globs?.length ? matchesAny(f, opts.globs) : opts.e2e ? isE2eFile(cfg, f) : isTestFile(cfg, f)));
  const sized = pick.map((f) => { let n = 1; try { n = lineCount(readFileSync(join(cwd, f), 'utf8')); } catch { /* unreadable: weight 1 */ } return { f, n }; });
  sized.sort((a, b) => b.n - a.n || a.f.localeCompare(b.f));
  const load = Array.from({ length: total }, () => ({ n: 0, files: [] as string[] }));
  for (const s of sized) {
    const lightest = load.reduce((m, x) => (x.n < m.n ? x : m), load[0]);
    lightest.n += s.n; lightest.files.push(s.f);
  }
  return load[index - 1].files.sort();
}
