import { importRepo } from './helpers/cuj.js';
import { j as journey, cfg, importRefusals } from './fixtures/cuj/cases.js';
const j = (fields: object = {}) => ({ id: 'CUJ-CONTACT', ...journey(), ...fields });
import { expect, it } from 'vitest';
import { mergeJourneys } from '../src/journeys-import.js';

it.each(['inferred', 'client-provided', 'template'])('CUJ-05 imports %s atomically and exact repeats write nothing', (provenance) => {
  const r = importRepo([j({ provenance })]);
  expect(r.import()).toEqual(['CUJ-CONTACT']);
  const saved = r.read();
  expect(JSON.parse(saved).journeys.items['CUJ-CONTACT']).toEqual({ ...j({ provenance, id: undefined }), approved: false, specs: [], routes: [] });
  expect(r.import()).toEqual([]);
  expect(r.read()).toBe(saved);
});
it.each(importRefusals)('CUJ-05 refuses invalid or protected imports without writing %j', ({ input, config, error }) => {
  const r = importRepo(input, config), before = r.read();
  expect(() => r.import()).toThrow(error);
  expect(r.read()).toBe(before);
});
it('CUJ-05 regenerates inferred journeys while preserving unrelated config and input', () => {
  const first = mergeJourneys(cfg, [j()]);
  const second = mergeJourneys(first, [j({ goal: 'Call now' })]);
  expect(second.journeys.items['CUJ-CONTACT'].goal).toBe('Call now');
  expect(first.journeys.items['CUJ-CONTACT'].goal).toBeUndefined();
  expect(second.journeys.trustedProducers).toEqual(['director']);
  expect(cfg.journeys.items).toEqual({});
});
it('CUJ-05 CLI imports for agents and returns exit 1 on refusal; hook permits import only', () => {
  const r = importRepo([j()], cfg, [j({ approved: true })]);
  const good = r.cli('input.json');
  expect([good.status, good.stderr]).toEqual([0, '']);
  const before = r.read();
  const bad = r.cli('bad.json');
  expect(bad.status).toBe(1);
  expect(bad.stderr).toContain('cannot approve');
  expect(r.read()).toBe(before);
  expect(['import input.json', 'approve CUJ-CONTACT', 'draft'].map(r.allows)).toEqual([true, false, false]);
});
