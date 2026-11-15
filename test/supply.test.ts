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
};
const calls: string[] = [];
const fake: Fetch = async (url, init) => {
  calls.push(url);
  const json = (body: unknown, status = 200) => ({ ok: status === 200, status, json: async () => body });
  if (url.endsWith('/bulk')) {
    const name = Object.keys(JSON.parse(init?.body ?? '{}'))[0];
    return json({ [name]: npm[name]?.advisories ?? [] });
  }
  const name = decodeURIComponent(url.split('/').pop() ?? '');
  if (url.includes('/downloads/')) return json({ downloads: npm[name]?.downloads });
  return npm[name] ? json({ time: { created: npm[name].created }, 'dist-tags': { latest: '1.0.0' } }) : json({}, 404);
};

describe('REG-01 new dependencies are checked against npm (slopsquatting)', () => {
  it('blocks missing and critically vulnerable packages, warns on young or unpopular ones, and caches', async () => {
    const r = repo();
    const out = await checkRegistry(r.ctx(), ['zod', 'reakt-dom-utilz', 'fresh-lib', 'old-vuln'], { fetch: fake, now: NOW, offline: false });
    expect(out.v.map((v) => `${v.maxMode ?? 'block'} ${v.message.split('.')[0]}`)).toEqual([
      'block "reakt-dom-utilz" does not exist on npm',
      'warn "fresh-lib" was first published 7 days ago (min 30)',
      'warn "fresh-lib" has 50 weekly downloads (min 1000)',
      'block "old-vuln" latest version has a critical advisory: RCE',
    ]);
    const before = calls.length;
    await checkRegistry(r.ctx(), ['zod'], { fetch: fake, now: NOW + 1000, offline: false });
    expect(calls.length).toBe(before);
  });

  it('skips with a metric when offline', async () => {
    const out = await checkRegistry(repo().ctx(), ['zod'], { fetch: fake, offline: true });
    expect(out).toEqual({ v: [], metrics: [{ name: 'deps.registry', value: 'skipped', detail: 'offline: zod not checked against npm' }] });
  });
});

describe('SCOPE-01 PRs that sprawl beyond their task warn', () => {
  it('warns above maxProductLines and on files outside the specs paths', () => {
    const cfg = { ...BASE_CONFIG, scope: { maxProductLines: 3 }, specs: { items: { 'CORE-01': { tier: 'T1', behaviour: 'core adds numbers', paths: ['src/core/**'] } } } };
    const r = repo({}, cfg).commit();
    r.write({ 'src/core/add.ts': 'export const a = 1;\n', 'src/lib/other.ts': 'a\nb\nc\n', 'src/core/add.test.ts': "it('CORE-01 a', () => { expect(1).toBe(1) });\n" });
    const out = checkScope(r.ctx('main'));
    expect(out.violations.map((v) => `${v.maxMode} ${v.file} ${v.message.split(' (')[0]}`)).toEqual([
      'warn (diff) 4 product lines changed',
      "warn src/lib/other.ts changed outside the paths of this task's specs",
    ]);
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
