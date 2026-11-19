import { runCommand } from './checks/run.js';

export interface SmokeOptions { sitemap?: boolean; fetch?: typeof fetch }

export interface SmokeResult { ok: boolean; lines: string[] }

/** guardrails smoke: read-only GETs. Each path must answer < 400. SMOKE_URL overrides --url (e.g. a preview). */
export async function smoke(baseUrl: string, paths: string[], timeoutMs = 15_000, options: SmokeOptions = {}): Promise<SmokeResult> {
  const lines: string[] = [];
  let ok = true;
  const request = options.fetch ?? fetch;
  const targets = options.sitemap ? [...new Set(paths.length ? paths : ['/'])] : (paths.length ? paths : ['/']);
  if (options.sitemap) {
    try {
      for (const path of await sitemapPaths(baseUrl, timeoutMs, request)) if (!targets.some((p) => new URL(p, baseUrl).href === new URL(path, baseUrl).href)) targets.push(path);
    } catch (e) {
      ok = false;
      lines.push(`FAIL sitemap: ${(e as Error).message}`);
    }
  }
  for (const p of targets) {
    const url = new URL(p, baseUrl).toString();
    const t = Date.now();
    try {
      const r = await request(url, { method: 'GET', redirect: 'follow', signal: AbortSignal.timeout(timeoutMs) });
      const pass = r.status < 400;
      ok &&= pass;
      lines.push(`${pass ? 'ok  ' : 'FAIL'} ${r.status} ${url} (${Date.now() - t}ms)`);
    } catch (e) {
      ok = false;
      lines.push(`FAIL --- ${url} (${(e as Error).name})`);
    }
  }
  return { ok, lines };
}

/** Discover canonical same-origin URLs, fetching their paths from the preview origin. */
async function sitemapPaths(baseUrl: string, timeoutMs: number, request: typeof fetch): Promise<string[]> {
  const base = new URL(baseUrl);
  const get = (path: string) => request(new URL(base.origin + path), { redirect: 'follow', signal: AbortSignal.timeout(timeoutMs) });
  let xml: string | undefined;
  for (const path of ['/sitemap-index.xml', '/sitemap.xml']) {
    try {
      const response = await get(path);
      if (response.status < 400) { xml = await response.text(); break; }
    } catch { /* Try the other conventional sitemap location. */ }
  }
  if (xml === undefined) throw new Error('neither /sitemap-index.xml nor /sitemap.xml is available');
  let canonicalOrigin: string | undefined;
  const visited = new Set<string>();
  const paths = new Set<string>();
  const read = async (body: string): Promise<void> => {
    if (!/<(?:sitemapindex|urlset)\b/i.test(body)) throw new Error('invalid sitemap XML');
    const index = /<sitemapindex\b/i.test(body);
    for (const match of body.matchAll(/<loc\b[^>]*>([\s\S]*?)<\/loc\s*>/gi)) {
      const value = match[1].trim().replace(/^<!\[CDATA\[([\s\S]*)\]\]>$/, '$1').replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&apos;/g, "'");
      const url = new URL(value, base.origin);
      if (!['http:', 'https:'].includes(url.protocol)) continue;
      // Astro emits production URLs even when its build is served on localhost.
      canonicalOrigin ??= url.origin;
      if (url.origin !== canonicalOrigin) continue;
      const path = url.pathname + url.search;
      if (!index) { paths.add(path); continue; }
      if (visited.has(path)) continue;
      if (visited.size >= 100) throw new Error('too many sitemap documents');
      visited.add(path);
      const response = await get(path);
      if (response.status >= 400) throw new Error(`${path} returned ${response.status}`);
      await read(await response.text());
    }
  };
  await read(xml);
  if (!paths.size) throw new Error('sitemap contains no same-origin page URLs');
  return [...paths].map((path) => base.origin + path);
}

/** guardrails secrets-check: run a provider's list command and check each required name appears. Output is never printed. */
export function secretsCheck(required: string[], listCommand: string, cwd: string): { ok: boolean; missing: string[] } {
  const r = runCommand(listCommand, cwd, {}, 120_000);
  if (r.code !== 0) return { ok: false, missing: ['(list command failed; check you are signed in to the provider)'] };
  const out = r.stdout;
  const missing = required.filter((n) => !new RegExp(`(^|[^A-Z0-9_])${n}([^A-Z0-9_]|$)`).test(out));
  return { ok: missing.length === 0, missing };
}
