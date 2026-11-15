import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import type { CheckContext, Metric, Violation } from '../types.js';
import { violation } from '../util.js';

export type Fetch = (url: string, init?: { method?: string; headers?: Record<string, string>; body?: string }) => Promise<{ ok: boolean; status: number; json(): Promise<unknown> }>;

export interface PackageFacts { exists: boolean; createdAt?: string; weeklyDownloads?: number; advisories?: { severity: string; title: string }[]; fetchedAt: number }

const CACHE = 'node_modules/.cache/guardrails/npm-registry.json';
const enc = (name: string) => name.replace('/', '%2F');

async function lookup(name: string, fetchImpl: Fetch, now: number): Promise<PackageFacts> {
  const meta = await fetchImpl(`https://registry.npmjs.org/${enc(name)}`, { headers: { accept: 'application/json' } });
  if (meta.status === 404) return { exists: false, fetchedAt: now };
  if (!meta.ok) throw new Error(`registry ${meta.status}`);
  const m = (await meta.json()) as { time?: { created?: string }; 'dist-tags'?: { latest?: string } };
  const dl = await fetchImpl(`https://api.npmjs.org/downloads/point/last-week/${name}`);
  const downloads = dl.ok ? ((await dl.json()) as { downloads?: number }).downloads : undefined;
  let advisories: PackageFacts['advisories'] = [];
  const latest = m['dist-tags']?.latest;
  if (latest) {
    const adv = await fetchImpl('https://registry.npmjs.org/-/npm/v1/security/advisories/bulk', {
      method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ [name]: [latest] }),
    });
    if (adv.ok) advisories = (((await adv.json()) as Record<string, { severity: string; title: string }[]>)[name] ?? []).map((a) => ({ severity: a.severity, title: a.title }));
  }
  return { exists: true, createdAt: m.time?.created, weeklyDownloads: downloads, advisories, fetchedAt: now };
}

/**
 * ARCH-4 (registry part): agents hallucinate package names (slopsquatting). Every newly added
 * dependency must exist on npm, be older than minAgeDays, have real usage and no known advisories.
 */
export interface RegistryDeps { fetch?: Fetch; now?: number; offline?: boolean }
export async function checkRegistry(ctx: CheckContext, names: string[], deps: RegistryDeps = {}): Promise<{ v: Violation[]; metrics: Metric[] }> {
  const fetchImpl = deps.fetch ?? (globalThis.fetch as unknown as Fetch | undefined);
  const now = deps.now ?? Date.now();
  const offline = deps.offline ?? process.env.GUARDRAILS_OFFLINE === '1';
  const r = ctx.config.deps.registry;
  if (!names.length || !r.enabled) return { v: [], metrics: [] };
  if (offline || !fetchImpl)
    return { v: [], metrics: [{ name: 'deps.registry', value: 'skipped', detail: `offline: ${names.join(', ')} not checked against npm` }] };
  const cachePath = join(ctx.cwd, CACHE);
  let cache: Record<string, PackageFacts> = {};
  try { if (existsSync(cachePath)) cache = JSON.parse(readFileSync(cachePath, 'utf8')); } catch { cache = {}; }
  const v: Violation[] = [];
  const failed: string[] = [];
  for (const name of [...new Set(names)]) {
    let facts = cache[name];
    if (!facts || now - facts.fetchedAt > r.cacheHours * 3_600_000) {
      try {
        facts = await lookup(name, fetchImpl, now);
        cache[name] = facts;
      } catch {
        failed.push(name);
        continue;
      }
    }
    const warn = (msg: string, fp: string) => v.push({ ...violation('ARCH-4', 'package.json', msg, fp), maxMode: 'warn' });
    if (!facts.exists) {
      v.push(violation('ARCH-4', 'package.json', `"${name}" does not exist on npm. Possibly a hallucinated package name; anyone could register it later.`, `npm-missing:${name}`));
      continue;
    }
    const ageDays = facts.createdAt ? (now - Date.parse(facts.createdAt)) / 86_400_000 : Infinity;
    if (ageDays < r.minAgeDays) warn(`"${name}" was first published ${Math.floor(ageDays)} days ago (min ${r.minAgeDays}). New packages are a supply-chain risk.`, `npm-young:${name}`);
    if (facts.weeklyDownloads !== undefined && facts.weeklyDownloads < r.minWeeklyDownloads) warn(`"${name}" has ${facts.weeklyDownloads} weekly downloads (min ${r.minWeeklyDownloads}).`, `npm-unpopular:${name}`);
    for (const a of facts.advisories ?? []) {
      const msg = `"${name}" latest version has a ${a.severity} advisory: ${a.title}`;
      if (a.severity === 'high' || a.severity === 'critical') v.push(violation('ARCH-4', 'package.json', msg, `npm-advisory:${name}:${a.title}`));
      else warn(msg, `npm-advisory:${name}:${a.title}`);
    }
  }
  try { mkdirSync(dirname(cachePath), { recursive: true }); writeFileSync(cachePath, JSON.stringify(cache)); } catch { /* read-only checkout */ }
  const metrics: Metric[] = [{ name: 'deps.registry', value: names.length - failed.length, detail: failed.length ? `could not reach npm for ${failed.join(', ')}` : 'checked against npm' }];
  return { v, metrics };
}
