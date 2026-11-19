import { expect, it } from 'vitest';
import { checkJourneys, isJourneyEnforced, journeyMatrix } from '../src/checks/journeys.js';
import { repo } from './helpers/repo.js';

it.each([
  [true, 'inferred', true, undefined, 'missing'], [false, 'client-provided', true, undefined, 'missing'],
  [false, 'inferred', false, 'warn', 'inferred'], [false, 'template', false, 'warn', 'inferred'],
  [false, undefined, false, null, 'draft'],
] as const)('CUJ-03 approval %s provenance %s controls enforcement', (approved, provenance, enforced, mode, status) => {
  const j = { title: 'Visitor contacts the business', priority: 'P0', steps: ['Open contact'], approved, ...(approved ? { approvedBy: 'Josh' } : {}), ...(provenance ? { provenance } : {}) };
  const r = repo({}, { version: 1, journeys: { items: { 'CUJ-CONTACT': j } } });
  expect(isJourneyEnforced(r.ctx().config.journeys.items['CUJ-CONTACT'])).toBe(enforced);
  expect(journeyMatrix(r.ctx())[0].status).toBe(status);
  const violations = checkJourneys(r.ctx()).violations;
  expect(violations.map((v) => v.maxMode)).toEqual(mode === null ? [] : [mode]);
  if (mode === 'warn') expect(violations[0].message).toContain('only warn until a human approves');
});
it.each(['inferred', 'template', 'client-provided'] as const)('CUJ-03 duplicates and failures for %s follow policy', (provenance) => {
  const r = repo({ 'e2e/a.spec.ts': "test('CUJ-CONTACT first', () => {});\ntest('CUJ-CONTACT second', () => {});", 'pw.json': JSON.stringify({ suites: [{ title: 'contact', specs: [{ title: 'CUJ-CONTACT fails', ok: false }] }] }) }, { version: 1, journeys: { resultsFile: 'pw.json', items: { 'CUJ-CONTACT': { title: 'Visitor contacts the business', priority: 'P0', steps: ['Open contact'], provenance } } } });
  const violations = checkJourneys(r.ctx()).violations;
  expect(violations.map((v) => [v.rule, v.maxMode])).toEqual([['JRN-1', provenance === 'client-provided' ? undefined : 'warn'], ['JRN-1', provenance === 'client-provided' ? undefined : 'warn']]);
  expect(violations[0].message).toContain('2 e2e tests');
  expect(violations[1].message).toContain('e2e test failed');
});
it('CUJ-03 client P1 failure warns even when no indexed test exists', () => {
  const r = repo({ 'pw.json': JSON.stringify({ suites: [{ title: '', specs: [{ title: 'CUJ-CONTACT failed', ok: false }] }] }) }, { version: 1, journeys: { resultsFile: 'pw.json', items: { 'CUJ-CONTACT': { title: 'Visitor contacts the business', priority: 'P1', steps: ['Open contact'], provenance: 'client-provided' } } } });
  expect(checkJourneys(r.ctx()).violations.map((v) => v.maxMode)).toEqual(['warn', 'warn']);
});
