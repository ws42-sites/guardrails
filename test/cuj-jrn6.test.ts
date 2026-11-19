import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { checkJourneys } from '../src/checks/journeys.js';
import { generateTests } from '../src/cuj-generate.js';
import { repo } from './helpers/repo.js';
const item = { title: 'Visitor contacts business', priority: 'P0', steps: ['Open contact'], run: [{ goto: '/' }] };
it.each([
  [true, 'inferred', undefined], [false, 'client-provided', undefined], [false, 'inferred', 'warn'], [false, 'template', 'warn'],
] as const)('CUJ-09 detects edited spec and runtime with approval %s provenance %s', (approved, provenance, maxMode) => {
  const r = repo({}, { version: 1, journeys: { items: { 'CUJ-CONTACT': { ...item, approved, ...(approved ? { approvedBy: 'Josh' } : {}), provenance } } } });
  const findings = () => checkJourneys(r.ctx()).violations.filter((v) => v.rule === 'JRN-6');
  expect(findings()).toEqual([]);
  generateTests(r.dir, r.ctx().config);
  expect(findings()).toEqual([]);
  for (const file of ['e2e/cuj/CUJ-CONTACT.spec.ts', 'e2e/cuj/_runtime.ts']) {
    r.write({ [file]: readFileSync(join(r.dir, file), 'utf8') + '// hand edit\n' });
    expect(findings().map((v) => [v.maxMode, v.message])).toEqual([[maxMode, 'CUJ-CONTACT: hand-edited or stale generated CUJ test; regenerate with guardrails journeys generate-tests']]);
    generateTests(r.dir, r.ctx().config);
  }
  r.remove('e2e/cuj/_runtime.ts');
  expect(findings()).toHaveLength(1);
});
it('CUJ-09 catches stale resolved data and reports one unresolved-token finding even with no spec', () => {
  const r = repo({ '.guardrails/client-data.json': '{"path":"/old"}' }, { version: 1, journeys: { items: { 'CUJ-CONTACT': { ...item, provenance: 'client-provided', run: [{ goto: '{{path}}' }] } } } });
  const findings = () => checkJourneys(r.ctx()).violations.filter((v) => v.rule === 'JRN-6');
  generateTests(r.dir, r.ctx().config);
  r.write({ '.guardrails/client-data.json': '{"path":"/new"}' });
  expect(findings()[0].message).toContain('stale generated CUJ test');
  r.remove('e2e/cuj/CUJ-CONTACT.spec.ts');
  expect(findings()).toEqual([]);
  r.remove('.guardrails/client-data.json');
  expect(findings().map((v) => v.message)).toEqual(['CUJ-CONTACT: Unresolved or empty tokens: path']);
});
