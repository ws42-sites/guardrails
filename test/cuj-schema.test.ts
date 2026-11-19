import { expect, it } from 'vitest';
import { journeySchema, journeyStepSchema, parseConfig } from '../src/config.js';

const journey = { title: 'Visitor contacts the business', priority: 'P0', steps: ['Open contact'] };
it('CUJ-01 preserves legacy provenance and defaults only sourced journeys', () => {
  expect(journeySchema.parse(journey).provenance).toBeUndefined();
  expect(journeySchema.parse({ ...journey, source: { by: 'director' } }).provenance).toBe('inferred');
  const input = { ...journey, source: { by: 'intake', runId: 'run-1', inputs: ['brief'] }, provenance: 'client-provided', persona: 'Customer', goal: 'Call {{client.phone}}', success: 'Connected' };
  expect(journeySchema.parse(input)).toEqual({ ...input, approved: false, specs: [], routes: [] });
  expect(parseConfig({ version: 1 }).journeys.trustedProducers).toEqual([]);
});
it.each([
  { goto: '/contact' }, { click: { role: 'button', name: 'Send' } }, { click: { text: 'Call' } },
  { fill: { label: 'Phone', value: '{{client.phone}}' } }, { submit: 'form > button[type="submit"]' },
  { expectText: 'Thank you' }, { expectUrl: '/thanks' }, { expectVisible: { text: 'Sent' } },
  { expectHref: { selector: 'a[href^="tel:"]', value: 'tel:{{client.phone}}' } },
  { expectNoHorizontalScroll: true }, { viewport: { width: 200, height: 4000 } },
  { mockRoute: { match: '/api/contact', status: 599 } }, { goto: '{{client.path}}' },
])('CUJ-01 accepts closed step %j', (step) => {
  expect(journeyStepSchema.parse(step)).toEqual(step);
});
it.each([
  { evaluate: 'alert(1)' }, { goto: '/ok', expectText: 'extra' }, { click: { selector: '#raw' } },
  { click: { role: 'button' } }, { expectVisible: { name: 'alone' } }, { fill: { label: 'X', value: '' } },
  { goto: 'https://example.com' }, { expectUrl: 'relative' }, { expectText: 'x'.repeat(301) },
  { expectText: ' ' }, { submit: 'a,button' }, { submit: 'a\\b' }, { submit: 'button;eval()' },
  { expectHref: { selector: 'a+div', value: '/ok' } }, { expectNoHorizontalScroll: false },
  { viewport: { width: 199, height: 400 } }, { viewport: { width: 400, height: 4001 } },
  { viewport: { width: 200.5, height: 400 } }, { mockRoute: { match: '/api', status: 99 } },
  { mockRoute: { match: '/api', status: 600 } }, { mockRoute: { match: '/api', status: 200.5 } },
  { fill: { label: 'X', value: 'Y', js: 'bad' } },
])('CUJ-01 rejects unsafe or malformed step %j', (step) => {
  expect(journeyStepSchema.safeParse(step).success).toBe(false);
});
it.each([
  { run: Array.from({ length: 41 }, () => ({ goto: '/' })) }, { source: { by: 'x', extra: true } },
  { source: { by: '{{client.producer}}' } }, { provenance: 'unknown' }, { persona: '' }, { goal: 'x'.repeat(301) },
  { surprise: true },
])('CUJ-01 rejects invalid journey fields %j', (fields) => {
  expect(journeySchema.safeParse({ ...journey, ...fields }).success).toBe(false);
});
