import { describe, expect, it } from 'vitest';
import { checkRegistry, type Fetch } from '../src/checks/registry.js';
import { checkScope } from '../src/checks/scope.js';
import { checkTests, compareConfigs } from '../src/checks/tests.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const NOW = Date.parse('2026-06-01T00:00:00Z');
const npm: Record<string, { created: string; downloads: number; advisories?: { severity: string; title: string }[] }> = {
  zod: { created: '2020-03-07T00:00:00Z', downloads: 30_000_000 },
  'fresh-lib': { created: '2026-05-25T00:00:00Z', downloads: 50 },
  'old-vuln': { created: '2015-01-01T00:00:00Z', downloads: 90_000, advisories: [{ severity: 'critical', title: 'RCE' }] },
  meh: { created: '2026-05-02T00:00:00Z', downloads: 1000, advisories: [{ severity: 'high', title: 'proto pollution' }, { severity: 'moderate', title: 'ReDoS' }] },
  '@scope/edge': { created: '2019-01-01T00:00:00Z', downloads: 5000 },
};
const calls: string[] = [];
const fake: Fetch = async (url, init) => {
  calls.push(init?.method === 'POST' ? `POST ${url} ${init.body}` : url);
  if (init?.method !== 'POST' && url.includes('registry.npmjs.org/') && init?.headers?.accept !== 'application/json') return { ok: false, status: 400, json: async () => ({}) };
  const json = (body: unknown, status = 200) => ({ ok: status === 200, status, json: async () => body });
  if (url.endsWith('/bulk')) {
    const name = Object.keys(JSON.parse(init?.body ?? '{}'))[0];
    return json({ [name]: npm[name]?.advisories ?? [] });
  }
  const name = decodeURIComponent(url.replace(/^https:\/\/[^/]+\/(downloads\/point\/last-week\/)?/, ''));
  if (url.includes('/downloads/')) return json({ downloads: npm[name]?.downloads });
  return npm[name] ? json({ time: { created: npm[name].created }, 'dist-tags': { latest: '1.0.0' } }) : json({}, 404);
};

describe('REG-01 new dependencies are checked against npm (slopsquatting)', () => {
  it('blocks missing and critically vulnerable packages, warns on young, unpopular or moderately vulnerable ones', async () => {
    const out = await checkRegistry(repo().ctx(), ['zod', 'reakt-dom-utilz', 'fresh-lib', 'old-vuln', 'meh', '@scope/edge'], { fetch: fake, now: NOW, offline: false });
    expect(out.v.map((v) => [v.maxMode ?? 'block', v.file, v.fingerprint.length, v.message])).toEqual([
      ['block', 'package.json', 12, '"reakt-dom-utilz" does not exist on npm. Possibly a hallucinated package name; anyone could register it later.'],
      ['warn', 'package.json', 12, '"fresh-lib" was first published 7 days ago (min 30). New packages are a supply-chain risk.'],
      ['warn', 'package.json', 12, '"fresh-lib" has 50 weekly downloads (min 1000).'],
      ['block', 'package.json', 12, '"old-vuln" latest version has a critical advisory: RCE'],
      ['block', 'package.json', 12, '"meh" latest version has a high advisory: proto pollution'],
      ['warn', 'package.json', 12, '"meh" latest version has a moderate advisory: ReDoS'],
    ]);
    expect(out.metrics).toEqual([{ name: 'deps.registry', value: 6, detail: 'checked against npm' }]);
    expect(calls.filter((c) => c.includes('scope')).sort()).toEqual([
      'POST https://registry.npmjs.org/-/npm/v1/security/advisories/bulk {"@scope/edge":["1.0.0"]}',
      'https://api.npmjs.org/downloads/point/last-week/@scope/edge',
      'https://registry.npmjs.org/@scope%2Fedge',
    ]);
  });

  it('caches lookups for cacheHours, reports unreachable packages, and skips when offline', async () => {
    const r = repo();
    await checkRegistry(r.ctx(), ['zod'], { fetch: fake, now: NOW, offline: false });
    const before = calls.length;
    await checkRegistry(r.ctx(), ['zod'], { fetch: fake, now: NOW + 24 * 3_600_000, offline: false });
    expect(calls.length).toBe(before);
    await checkRegistry(r.ctx(), ['zod'], { fetch: fake, now: NOW + 24 * 3_600_000 + 1, offline: false });
    expect(calls.length).toBe(before + 3);
    const down: Fetch = async () => ({ ok: false, status: 503, json: async () => ({}) });
    expect(await checkRegistry(r.ctx(), ['nope'], { fetch: down, now: NOW, offline: false })).toEqual({ v: [], metrics: [{ name: 'deps.registry', value: 0, detail: 'could not reach npm for nope' }] });
    expect(await checkRegistry(r.ctx(), ['zod'], { fetch: fake, offline: true })).toEqual({ v: [], metrics: [{ name: 'deps.registry', value: 'skipped', detail: 'offline: zod not checked against npm' }] });
    expect(await checkRegistry(r.ctx(), [], { fetch: fake, offline: false })).toEqual({ v: [], metrics: [] });
  });
});

describe('SCOPE-01 PRs that sprawl beyond their task warn', () => {
  const cfg = (cap: number) => ({ ...BASE_CONFIG, scope: { maxProductLines: cap }, specs: { items: { 'CORE-01': { tier: 'T1', behaviour: 'core adds numbers', paths: ['src/core/**'] }, 'LIB-01': { tier: 'T2', behaviour: 'lib' } } } });
  it.each([
    [3, ['warn src/lib/other.ts changed outside the paths of this task\'s specs (CORE-01). Keep the PR to the task or flag the spec.']],
    [2, ['warn (diff) 3 product lines changed (cap 2). Split the PR so each part can be reviewed.', 'warn src/lib/other.ts changed outside the paths of this task\'s specs (CORE-01). Keep the PR to the task or flag the spec.']],
  ])('cap %i', (cap, expected) => {
    const r = repo({ 'src/lib/other.ts': 'a\nb\n' }, cfg(cap)).commit();
    r.write({ 'src/core/add.ts': 'export const a = 1;\n', 'src/lib/other.ts': 'a\nc\n', 'src/core/add.test.ts': "it('CORE-01 a', () => { expect(1).toBe(1) });\n" });
    const out = checkScope(r.ctx('main'));
    expect(out.violations.map((v) => `${v.maxMode} ${v.file} ${v.message}`)).toEqual(expected);
    expect(out.metrics).toEqual([{ name: 'scope.productLines', value: 3, detail: 'specs: CORE-01' }]);
  });

  it('uses the task spec IDs passed in, and skips without a base', () => {
    const r = repo({}, cfg(600)).commit();
    r.write({ 'src/core/add.ts': 'export const a = 1;\n' });
    const ctx = r.ctx('main');
    ctx.options.specs = ['LIB-01'];
    expect(checkScope(ctx)).toEqual({ violations: [], metrics: [{ name: 'scope.productLines', value: 1, detail: 'specs: LIB-01' }] });
    expect(checkScope(r.ctx())).toEqual({ violations: [] });
  });
});

describe('WEAK-02 loosening the new guardrails settings counts as weakening', () => {
  it.each([
    [{ duplication: { maxPercent: 3 } }, { duplication: { maxPercent: 10 } }, ['duplication.maxPercent: 3 -> 10']],
    [{}, { duplication: { enabled: false } }, ['duplication.enabled: disabled']],
    [{ deps: { registry: { minAgeDays: 30 } } }, { deps: { registry: { minAgeDays: 1 } } }, ['deps.registry.minAgeDays: 30 -> 1']],
    [{ lock: { paths: ['a', 'b'] } }, { lock: { paths: ['a'] } }, ['lock.paths: removed "b"']],
    [{ specs: { items: { X: { tier: 'T1' } } } }, { specs: { items: { X: { tier: 'T1', regressionGuard: true } } } }, ['specs.items.X.regressionGuard: set (tests may pass on base)']],
    [{ scope: { maxProductLines: 600 } }, { scope: { maxProductLines: 400 } }, []],
    [{ mutation: { exclude: [] } }, { mutation: { exclude: ['src/a.ts'] } }, ['mutation.exclude: added "src/a.ts"']],
    [{ journeys: { items: { 'CUJ-A1': { approved: false } } } }, { journeys: { items: {} } }, []],
  ])('%j -> %j', (before, after, expected) => {
    expect(compareConfigs(before, after)).toEqual(expected);
  });
});

describe('WEAK-02 a code owner can approve a config loosening with a label', () => {
  it('blocks a new deps.allow entry unless the PR has guardrails:config-approved', () => {
    const r = repo().commit();
    r.write({ 'guardrails.config.json': JSON.stringify({ ...BASE_CONFIG, deps: { allow: ['jscpd'] } }) });
    const weak = (labels: string[]) => checkTests(r.ctx('main', labels)).violations.filter((v) => v.rule === 'TEST-7').map((v) => v.message.split('. ')[0]);
    expect([weak([]), weak(['guardrails:config-approved'])]).toEqual([['config loosened: deps.allow: added "jscpd"'], []]);
  });
});
