import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it, vi } from 'vitest';
import { runJourneys, scaffoldJourneys } from '../src/cuj-commands.js';
import { runCommand } from '../src/checks/run.js';
import { repo } from './helpers/repo.js';
vi.mock('../src/checks/run.js', () => ({ runCommand: vi.fn() }));
const item = { title: 'Visitor contacts business', priority: 'P0', steps: ['Open contact'], run: [{ goto: '/' }] };
function site() {
  return repo({ 'node_modules/@playwright/test/package.json': '{"main":"index.js"}', 'node_modules/@playwright/test/index.js': '' }, {
    version: 1, journeys: { resultsFile: 'results.json', items: {
      'CUJ-CLIENT': { ...item, provenance: 'client-provided' }, 'CUJ-INFERRED': { ...item, provenance: 'inferred' },
    } },
  });
}
it('CUJ-10 scaffolds required settings and never overwrites config or gitkeep', () => {
  const r = site();
  expect(scaffoldJourneys(r.dir)).toEqual(['playwright.cuj.config.ts', 'e2e/cuj/.gitkeep']);
  const config = readFileSync(join(r.dir, 'playwright.cuj.config.ts'), 'utf8');
  expect(config).toContain('baseURL: process.env.CUJ_BASE_URL');
  expect(config).toContain('fullyParallel: false');
  expect(config).toContain('workers: 2');
  expect(config).toContain('retries: 0');
  expect(config).toContain("trace: 'retain-on-failure'");
  expect(config).toContain("reporter: [['list'], ['json'");
  r.write({ 'playwright.cuj.config.ts': 'custom', 'e2e/cuj/.gitkeep': 'keep' });
  expect(scaffoldJourneys(r.dir)).toEqual([]);
  expect(readFileSync(join(r.dir, 'playwright.cuj.config.ts'), 'utf8')).toBe('custom');
  expect(readFileSync(join(r.dir, 'e2e/cuj/.gitkeep'), 'utf8')).toBe('keep');
});
it.each([[true, true, 0, 0], [true, false, 1, 0], [false, true, 1, 1], [false, false, 1, 1], [true, true, 130, 130]])('CUJ-10 summaries and exit policy client %s inferred %s runner %s', (client, inferred, runnerCode, code) => {
  const r = site(); scaffoldJourneys(r.dir);
  vi.mocked(runCommand).mockImplementation((command, cwd, env) => {
    expect(command).toContain('npx --no-install playwright test e2e/cuj --config playwright.cuj.config.ts');
    expect(cwd).toBe(r.dir);
    expect(env).toEqual({ CUJ_BASE_URL: 'https://preview.test', PLAYWRIGHT_JSON_OUTPUT_NAME: join(r.dir, 'results.json') });
    r.write({ 'results.json': JSON.stringify({ suites: [{ specs: [{ title: 'CUJ-CLIENT contact', ok: client }, { title: 'CUJ-INFERRED contact', ok: inferred }] }] }) });
    return { code: runnerCode, stdout: '', stderr: '' };
  });
  const result = runJourneys(r.dir, r.ctx().config, { baseUrl: 'https://preview.test' });
  expect(result).toEqual({ code, lines: [
    `CUJ-CLIENT P0 client-provided enforced yes ${client ? 'pass' : 'fail'}`,
    `${inferred ? '' : 'WARN '}CUJ-INFERRED P0 inferred enforced no ${inferred ? 'pass' : 'fail'}`,
  ] });
});
it('CUJ-10 filters selected IDs and enforced journeys without accidentally running everything', () => {
  const r = site(); scaffoldJourneys(r.dir);
  vi.mocked(runCommand).mockImplementation((command) => {
    expect(command).toContain('--grep "(?:^|\\.spec\\.[jt]s(?: \\u203A)? )(CUJ-CLIENT )"');
    r.write({ 'results.json': '{"suites":[{"specs":[{"title":"CUJ-CLIENT contact","ok":true}]}]}' });
    return { code: 0, stdout: '', stderr: '' };
  });
  expect(runJourneys(r.dir, r.ctx().config, { baseUrl: 'http://localhost:4321', onlyEnforced: true }).lines).toEqual(['CUJ-CLIENT P0 client-provided enforced yes pass']);
  expect(runJourneys(r.dir, r.ctx().config, { baseUrl: 'http://localhost:4321', ids: ['CUJ-CLIENT'] }).code).toBe(0);
  expect(runJourneys(r.dir, r.ctx().config, { baseUrl: 'http://localhost:4321', ids: ['CUJ-INFERRED'], onlyEnforced: true }).lines).toEqual(['journeys run: no selected executable journeys']);
});
it('CUJ-10 gives clear setup errors and rejects absent, stale and malformed reports', () => {
  const r = site(), opts = { baseUrl: 'https://preview.test' };
  expect(runJourneys(r.dir, r.ctx().config, opts)).toEqual({ code: 2, lines: ['journeys run: missing playwright.cuj.config.ts; run guardrails journeys scaffold'] });
  scaffoldJourneys(r.dir);
  r.remove('node_modules/@playwright/test/index.js');
  expect(runJourneys(r.dir, r.ctx().config, opts).lines[0]).toContain('missing @playwright/test');
  r.write({ 'node_modules/@playwright/test/index.js': '' });
  expect(runJourneys(r.dir, r.ctx().config, { baseUrl: 'oops' }).code).toBe(2);
  expect(runJourneys(r.dir, r.ctx().config, { ...opts, ids: ['CUJ-UNKNOWN'] }).lines).toEqual(['journeys run: unknown journey ID']);
  r.write({ 'results.json': '{"suites":[{"specs":[{"title":"CUJ-CLIENT contact","ok":true}]}]}' });
  vi.mocked(runCommand).mockReturnValue({ code: 1, stdout: '', stderr: 'failed' });
  expect(runJourneys(r.dir, r.ctx().config, opts)).toEqual({ code: 2, lines: ['journeys run: incomplete Playwright JSON report; runner failed or tests are missing'] });
  vi.mocked(runCommand).mockImplementation(() => { r.write({ 'results.json': 'broken' }); return { code: 1, stdout: '', stderr: '' }; });
  expect(runJourneys(r.dir, r.ctx().config, opts).lines).toEqual(['journeys run: invalid Playwright JSON report']);
});
