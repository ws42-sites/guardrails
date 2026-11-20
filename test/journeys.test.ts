import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { checkJourneys, journeyMatrix, routeOf } from '../src/checks/journeys.js';
import { parseConfig } from '../src/config.js';
import { addDrafts, approveJourney, draftJourneys, heuristicDrafts, presetDrafts } from '../src/journeys.js';
import { guard } from '../src/hook.js';
import { run } from '../src/runner.js';
import { toMarkdown, toText } from '../src/report.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const J = (over: object = {}) => ({ title: 'User signs in and sees the pipeline', priority: 'P0', steps: ['Sign in', 'See pipeline'], specs: ['CORE-01'], routes: ['/login', '/api/admin/*'], approved: true, approvedBy: 'josh', ...over });
const cfg = (items: object, extra: object = {}) => ({ ...BASE_CONFIG, journeys: { items, ...extra } });
const e2e = (title: string) => `import { test, expect } from '@playwright/test';\ntest('${title}', async ({ page }) => { await expect(page).toHaveURL('/'); });\n`;
const found = (vs: { rule: string; maxMode?: string; message: string }[]) => vs.map((v) => `${v.rule} ${v.maxMode ?? 'block'} ${v.message.split('. ')[0]}`);

describe('JOURNEY-01 approved P0/P1 journeys need exactly one e2e test', () => {
  it.each([
    ['P0', [], ['JRN-1 block CUJ-SIGNIN (P0) "User signs in and sees the pipeline" has no e2e test titled with its ID in e2e/**/*.spec.ts, e2e/**/*.test.ts.']],
    ['P1', [], ['JRN-1 warn CUJ-SIGNIN (P1) "User signs in and sees the pipeline" has no e2e test titled with its ID in e2e/**/*.spec.ts, e2e/**/*.test.ts.']],
    ['P2', [], []],
    ['P0', ['CUJ-SIGNIN signs in'], []],
    ['P0', ['CUJ-SIGNIN a', 'CUJ-SIGNIN b'], ['JRN-1 block CUJ-SIGNIN has 2 e2e tests (e2e/a.spec.ts:2, e2e/b.spec.ts:2); keep exactly one journey test and put variations in steps.']],
  ])('%s with e2e %j', (priority, titles, expected) => {
    const files = Object.fromEntries(titles.map((t, i) => [`e2e/${'ab'[i]}.spec.ts`, e2e(t)]));
    const r = repo({ ...files, 'src/core/a.test.ts': "it('CORE-01 a', () => { expect(1).toBe(1) });\n" }, cfg({ 'CUJ-SIGNIN': J({ priority }) }));
    expect(found(checkJourneys(r.ctx()).violations).filter((x) => x.startsWith('JRN-1'))).toEqual(expected);
  });

  it('builds the matrix with pass/fail from a Playwright JSON report, ignores drafts, and renders it in reports', async () => {
    const report = { suites: [{ title: 'a.spec.ts', specs: [{ title: 'CUJ-SIGNIN signs in', ok: false }] }] };
    const r = repo({ 'e2e/a.spec.ts': e2e('CUJ-SIGNIN signs in'), 'pw.json': JSON.stringify(report), 'src/core/a.test.ts': "it('CORE-01 a', () => { expect(1).toBe(1) });\n" },
      cfg({ 'CUJ-SIGNIN': J(), 'CUJ-DRAFT': J({ approved: false, approvedBy: undefined, specs: [] }) }, { resultsFile: 'pw.json' }));
    expect(journeyMatrix(r.ctx())).toEqual([
      { id: 'CUJ-SIGNIN', title: 'User signs in and sees the pipeline', priority: 'P0', approved: true, specs: [{ id: 'CORE-01', tests: 1 }], e2e: ['e2e/a.spec.ts:2'], status: 'fail' },
      { id: 'CUJ-DRAFT', title: 'User signs in and sees the pipeline', priority: 'P0', approved: false, specs: [], e2e: [], status: 'draft' },
    ]);
    const out = checkJourneys(r.ctx());
    expect([found(out.violations), out.metrics]).toEqual([
      ['JRN-1 block CUJ-SIGNIN e2e test failed in pw.json.', 'JRN-2 warn LIB-01 belongs to no approved journey'],
      [{ name: 'journeys', value: '1/1', detail: 'approved journeys with an e2e test (1 drafts)' }],
    ]);
    const rep = await run({ cwd: r.dir, knip: false, only: ['journeys'] });
    expect(toText(rep)).toContain('    ✖ CUJ-SIGNIN P0  specs: CORE-01(1)  e2e: e2e/a.spec.ts:2\n    ○ CUJ-DRAFT P0 draft  specs: -  e2e: -');
    expect(toMarkdown(rep)).toContain('| CUJ-SIGNIN User signs in and sees the pipeline | P0 | ✖ fail | CORE-01 (1) | `e2e/a.spec.ts:2` |');
  });
});

describe('JOURNEY-02 specs serve approved journeys; back-references are validated', () => {
  it('rejects unknown specs, bad IDs, unapproved approvals and mismatched back-references', () => {
    expect(() => parseConfig(cfg({ 'CUJ-X1': J({ specs: ['NOPE-01'] }) }))).toThrow('journeys.items.CUJ-X1.specs: unknown spec NOPE-01');
    expect(() => parseConfig(cfg({ 'JOURNEY-1': J() }))).toThrow('journey ID must match');
    expect(() => parseConfig(cfg({ 'CUJ-X1': J({ approvedBy: undefined }) }))).toThrow('approved journeys need approvedBy');
    const back = { ...cfg({ 'CUJ-X1': J({ specs: [] }) }), specs: { items: { 'CORE-01': { tier: 'T1', behaviour: 'core adds', journeys: ['CUJ-X1'] } } } };
    expect(() => parseConfig(back)).toThrow('CUJ-X1 does not list CORE-01 in its specs');
  });
});

describe('JOURNEY-03 journeys are human-owned', () => {
  it('flags added, changed and approved journeys in a PR unless a code owner labels it', () => {
    const r = repo({}, cfg({ 'CUJ-A1': J({ approved: false, approvedBy: undefined }) })).commit();
    r.write({ 'guardrails.config.json': JSON.stringify(cfg({ 'CUJ-A1': J(), 'CUJ-B1': J({ approved: false, approvedBy: undefined }) })) });
    const jrn3 = (labels: string[]) => checkJourneys(r.ctx('main', labels)).violations.filter((v) => v.rule === 'JRN-3').map((v) => v.message.split(' in this')[0]);
    expect([jrn3([]), jrn3(['guardrails:journeys-approved'])]).toEqual([['journey CUJ-A1 approved', 'journey CUJ-B1 added'], []]);
  });

  it('refuses approval from agents, records approvedBy and back-references, and hooks deny agent commands', () => {
    const r = repo({}, cfg({ 'CUJ-A1': J({ approved: false, approvedBy: undefined }) }));
    expect(() => approveJourney(r.dir, 'CUJ-A1', { by: 'bot', env: { GUARDRAILS_AGENT: 'claude-code' } })).toThrow('for humans only');
    expect(approveJourney(r.dir, 'CUJ-A1', { by: 'Josh', env: {} })).toBe('Josh');
    const saved = JSON.parse(readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8'));
    expect([saved.journeys.items['CUJ-A1'].approvedBy, saved.specs.items['CORE-01'].journeys]).toEqual(['Josh', ['CUJ-A1']]);
    const env = { CLAUDE_PROJECT_DIR: r.dir };
    expect(['npx guardrails journeys approve CUJ-A1', 'GUARDRAILS_AGENT= npx guardrails journeys list', 'unset GUARDRAILS_AGENT', 'npx guardrails journeys list']
      .map((c) => guard({ tool_input: { command: c } }, env).allow)).toEqual([false, false, false, true]);
  });
});

describe('JOURNEY-04 new routes and forms need a journey', () => {
  it('warns on uncovered new pages, endpoints, router paths and forms', () => {
    const r = repo({}, { ...cfg({ 'CUJ-A1': J({ routes: ['/login', '/api/admin/*'] }) }), sourceRoots: ['src', 'functions'] }).commit();
    r.write({
      'src/pages/login.tsx': 'export default () => <form />;\n',
      'src/pages/settings/[tab].tsx': 'export default () => <form />;\n',
      'functions/api/admin/users.ts': 'export const onRequest = () => {};\n',
      'functions/api/support.ts': 'export const onRequest = () => {};\n',
      'src/router.tsx': '<Route path="/pricing" element={<P />} />\n',
    });
    expect(found(checkJourneys(r.ctx('main')).violations).filter((x) => x.startsWith('JRN-4'))).toEqual([
      'JRN-4 warn adds route /api/support, which no journey covers',
      'JRN-4 warn adds route /settings/:tab, which no journey covers',
      'JRN-4 warn adds a form on /settings/:tab that no journey\'s routes cover',
      'JRN-4 warn adds route /pricing, which no journey covers',
    ]);
  });

  it.each([
    ['src/pages/index.tsx', '/'], ['app/blog/[slug]/page.tsx', '/blog/:slug'], ['functions/api/[[...path]].ts', '/api/*'], ['src/pages/about.astro', '/about'],
  ])('routeOf(%s) = %s', (file, route) => {
    expect(routeOf(file)).toBe(route);
  });
});

describe('JOURNEY-05 drafting journeys: LLM, heuristic fallback and the small-business preset', () => {
  it('drafts from routes without an API key, and from Gemini when GEMINI_API_KEY is set, always unapproved', async () => {
    const r = repo({ 'src/pages/contact.tsx': 'x', 'functions/api/contact.ts': 'x', 'README.md': '# Bakery site' }, { ...BASE_CONFIG, sourceRoots: ['src', 'functions'] });
    expect(heuristicDrafts(['/contact', '/api/contact'], 8)).toEqual({ 'CUJ-CONTACT': {
      title: 'User opens /contact and completes the contact task end to end', priority: 'P0', specs: [], routes: ['/contact', '/api/contact'], approved: false,
      steps: ['Open /contact', 'Complete the main contact action', 'See the result confirmed', 'Data reaches /api/contact'],
    } });
    const sent: string[] = [];
    const fake = async (url: string, init: { headers: Record<string, string>; body: string }) => {
      sent.push(`${url} ${init.headers['x-goog-api-key']} ${init.body.includes('# Bakery site')}`);
      const journeys = [{ id: 'cuj-contact', title: 'Visitor sends an enquiry and sees confirmation', priority: 'P0', steps: ['Open contact', 'Submit'], routes: ['/contact'], specs: ['CORE-01', 'FAKE-99'] }];
      return { ok: true, status: 200, json: async () => ({ candidates: [{ content: { parts: [{ text: JSON.stringify({ journeys }) }] } }] }) };
    };
    const res = await draftJourneys(r.dir, { env: { GEMINI_API_KEY: 'k' }, fetch: fake });
    expect([res.via, res.drafts, sent]).toEqual(['gemini:gemini-2.5-flash',
      { 'CUJ-CONTACT': { title: 'Visitor sends an enquiry and sees confirmation', priority: 'P0', steps: ['Open contact', 'Submit'], routes: ['/contact'], specs: ['CORE-01'], approved: false } },
      ['https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent k true']]);
    expect((await draftJourneys(r.dir, { env: {} })).via).toBe('heuristic (no GEMINI_API_KEY or OPENAI_API_KEY)');
  });

  it('pulls core small-business journeys and only the optional ones the site needs, as drafts', () => {
    const r = repo();
    const drafts = presetDrafts('small-business', ['src/pages/menu.tsx']);
    expect(Object.keys(drafts)).toEqual(['CUJ-SERVICES', 'CUJ-CONTACT', 'CUJ-BOOK', 'CUJ-TRUST', 'CUJ-MOBILE', 'CUJ-LOCATION', 'CUJ-MENU']);
    expect(addDrafts(r.dir, drafts)).toHaveLength(7);
    expect(addDrafts(r.dir, drafts)).toEqual([]);
    const items = JSON.parse(readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8')).journeys.items;
    expect(Object.values(items).every((j) => (j as { approved: boolean }).approved === false)).toBe(true);
  });
});
