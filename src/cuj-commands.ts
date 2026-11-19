import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import type { GuardrailsConfig } from './config.js';
import { e2eResults, isJourneyEnforced } from './checks/journeys.js';
import { runCommand } from './checks/run.js';

export function scaffoldJourneys(cwd: string): string[] {
  const files = {
    'playwright.cuj.config.ts': readFileSync(new URL('../template/site/playwright.cuj.config.ts', import.meta.url), 'utf8'),
    'e2e/cuj/.gitkeep': '',
  };
  const written: string[] = [];
  for (const [name, content] of Object.entries(files)) {
    const file = join(cwd, name);
    if (existsSync(file)) continue;
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, content);
    written.push(name);
  }
  return written;
}
export function runJourneys(cwd: string, cfg: GuardrailsConfig, opts: { baseUrl: string; ids?: string[]; onlyEnforced?: boolean }): { code: number; lines: string[] } {
  const fail = (message: string) => ({ code: 2, lines: [message] });
  try { const url = new URL(opts.baseUrl); if (!['http:', 'https:'].includes(url.protocol)) throw new Error(); }
  catch { return fail('journeys run: --base-url must be an http(s) URL'); }
  if (!existsSync(join(cwd, 'playwright.cuj.config.ts'))) return fail('journeys run: missing playwright.cuj.config.ts; run guardrails journeys scaffold');
  try { createRequire(join(resolve(cwd), 'package.json')).resolve('@playwright/test'); }
  catch { return fail('journeys run: missing @playwright/test; install it in the site first'); }
  if (opts.ids?.some((id) => !cfg.journeys.items[id])) return fail('journeys run: unknown journey ID');
  const selected = Object.entries(cfg.journeys.items).filter(([id, j]) => j.run?.length && (!opts.ids || opts.ids.includes(id)) && (!opts.onlyEnforced || isJourneyEnforced(j)));
  if (!selected.length) return { code: 0, lines: ['journeys run: no selected executable journeys'] };
  if (selected.some(([id]) => !/^[A-Za-z0-9_-]+$/.test(id))) return fail('journeys run: unsafe journey ID');
  const resultsFile = cfg.journeys.resultsFile ?? '.guardrails/journeys-results.json';
  const resultPath = resolve(cwd, resultsFile);
  mkdirSync(dirname(resultPath), { recursive: true });
  rmSync(resultPath, { force: true });
  // IDs are validated above before being interpolated into a shell argument.
  const grep = selected.map(([id]) => `${id} `).join('|');
  const result = runCommand(`npx --no-install playwright test e2e/cuj --config playwright.cuj.config.ts --reporter=list,json --grep "^(${grep})"`, cwd,
    { CUJ_BASE_URL: opts.baseUrl, PLAYWRIGHT_JSON_OUTPUT_NAME: resultPath });
  let results: Map<string, boolean>;
  try { results = e2eResults({ ...cfg, journeys: { ...cfg.journeys, resultsFile } }, cwd); }
  catch { return fail('journeys run: invalid Playwright JSON report'); }
  if (!existsSync(resultPath) || selected.some(([id]) => !results.has(id))) return fail('journeys run: incomplete Playwright JSON report; runner failed or tests are missing');
  const lines = selected.map(([id, j]) => `${results.get(id) === false && !isJourneyEnforced(j) ? 'WARN ' : ''}${id} ${j.priority} ${j.provenance ?? 'legacy'} enforced ${isJourneyEnforced(j) ? 'yes' : 'no'} ${results.get(id) ? 'pass' : 'fail'}`);
  const enforcedFailure = selected.some(([id, j]) => isJourneyEnforced(j) && !results.get(id));
  return { code: result.code > 1 ? result.code : enforcedFailure ? 1 : result.code && selected.every(([id]) => results.get(id)) ? 2 : 0, lines };
}
