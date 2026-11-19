import { validSteps, invalidSteps, invalidJourneyFields } from './fixtures/cuj/cases.js';
import { expect, it } from 'vitest';
import { journeySchema, journeyStepSchema, parseConfig } from '../src/config.js';

const run40 = Array(40).fill({ goto: '/' });
const journey = { title: 'Visitor contacts the business', priority: 'P0', steps: ['Open contact'] };
it('CUJ-01 preserves legacy provenance and defaults only sourced journeys', () => {
  expect(journeySchema.parse(journey).provenance).toBeUndefined();
  expect(journeySchema.parse({ ...journey, source: { by: 'director' } }).provenance).toBe('inferred');
  const input = { ...journey, source: { by: 'intake', runId: 'run-1', inputs: ['brief'] }, provenance: 'client-provided', persona: 'Customer', goal: 'Call {{client.phone}}', success: 'Connected' };
  expect(journeySchema.parse(input)).toEqual({ ...input, approved: false, specs: [], routes: [] });
  expect(parseConfig({ version: 1 }).journeys.trustedProducers).toEqual([]);
});
it.each(validSteps)('CUJ-01 accepts closed step %j', (step) =>
  expect(journeyStepSchema.parse(step)).toEqual(step));
it.each(invalidSteps)('CUJ-01 rejects unsafe or malformed step %j', (step) =>
  expect(journeyStepSchema.safeParse(step).success).toBe(false));
it.each(invalidJourneyFields)('CUJ-01 rejects invalid journey fields %j', (fields) =>
  expect(journeySchema.safeParse({ ...journey, ...fields }).success).toBe(false));
it.each([[run40, true], [[...run40, { goto: '/' }], false]] as const)('CUJ-01 run boundary %j accepts: %s', (run, accepted) =>
  expect(journeySchema.safeParse({ ...journey, run }).success).toBe(accepted));
