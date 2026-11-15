import { runCommand } from './checks/run.js';

export interface SmokeResult { ok: boolean; lines: string[] }

/** guardrails smoke: read-only GETs. Each path must answer < 400. SMOKE_URL overrides --url (e.g. a preview). */
export async function smoke(baseUrl: string, paths: string[], timeoutMs = 15_000): Promise<SmokeResult> {
  const lines: string[] = [];
  let ok = true;
  for (const p of paths.length ? paths : ['/']) {
    const url = new URL(p, baseUrl).toString();
    const t = Date.now();
    try {
      const r = await fetch(url, { method: 'GET', redirect: 'follow', signal: AbortSignal.timeout(timeoutMs) });
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

/** guardrails secrets-check: run a provider's list command and check each required name appears. Output is never printed. */
export function secretsCheck(required: string[], listCommand: string, cwd: string): { ok: boolean; missing: string[] } {
  const r = runCommand(listCommand, cwd, {}, 120_000);
  if (r.code !== 0) return { ok: false, missing: ['(list command failed; check you are signed in to the provider)'] };
  const out = r.stdout;
  const missing = required.filter((n) => !new RegExp(`(^|[^A-Z0-9_])${n}([^A-Z0-9_]|$)`).test(out));
  return { ok: missing.length === 0, missing };
}
