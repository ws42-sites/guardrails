import { expect, it, vi } from 'vitest';
import { checkJourneys, journeyOwnershipChanges } from '../src/checks/journeys.js';
import { showAt } from '../src/util.js';
import { repo } from './helpers/repo.js';

vi.mock('../src/util.js', async (original) => ({ ...await original<typeof import('../src/util.js')>(), mergeBase: () => 'base', showAt: vi.fn(), diffAgainst: () => ({}) }));
const j = (fields: object = {}) => ({ title: 'Visitor contacts the business', priority: 'P0', steps: ['Open contact'], provenance: 'inferred', source: { by: 'director' }, run: [{ goto: '/contact' }], ...fields });
const block = (item: unknown, trust = ['director']) => ({ trustedProducers: trust, items: item ? { 'CUJ-CONTACT': item } : {} });
it.each([
  ['inferred addition', undefined, j(), false],
  ['client addition', undefined, j({ provenance: 'client-provided' }), false],
  ['template addition', undefined, j({ provenance: 'template', approved: false }), false],
  ['untrusted addition', undefined, j({ source: { by: 'stranger' } }), true],
  ['unsourced addition', undefined, j({ source: undefined }), true],
  ['missing provenance addition', undefined, j({ provenance: undefined }), true],
  ['missing run addition', undefined, j({ run: undefined }), true],
  ['invalid run addition', undefined, j({ run: [{ eval: 'bad' }] }), true],
  ['approved addition', undefined, j({ approved: true, approvedBy: 'Josh' }), true],
  ['trusted regeneration', j(), j({ goal: 'Updated goal' }), false],
  ['trusted removal', j(), undefined, false],
  ['template removal', j({ provenance: 'template' }), undefined, false],
  ['untrusted regeneration', j({ source: { by: 'stranger' } }), j(), true],
  ['untrusted removal', j({ source: { by: 'stranger' } }), undefined, true],
  ['approved edit', j({ approved: true, approvedBy: 'Josh' }), j({ goal: 'New' }), true],
  ['approved removal', j({ approved: true, approvedBy: 'Josh' }), undefined, true],
  ['client edit', j({ provenance: 'client-provided' }), j({ goal: 'New' }), true],
  ['client removal', j({ provenance: 'client-provided' }), undefined, true],
  ['approval flip', j(), j({ approved: true, approvedBy: 'Josh' }), true],
  ['client promotion', j(), j({ provenance: 'client-provided' }), true],
  ['producer substitution', j(), j({ source: { by: 'stranger' } }), true],
  ['legacy edit', j({ source: undefined, provenance: undefined }), j(), true],
  ['unchanged', j(), j(), false],
] as const)('CUJ-04 ownership table: %s', (_name, before, after, blocked) => {
  const changes = journeyOwnershipChanges(block(before), block(after));
  expect(changes.length).toBe(blocked ? 1 : 0);
  if (blocked) expect(changes[0].message).toContain('journey CUJ-CONTACT');
});
it.each(['trustedProducers', 'idPattern', 'e2eGlobs'] as const)('CUJ-04 policy change %s requires approval even with no items', (key) => {
  const next = { ...block(undefined), [key]: key === 'idPattern' ? '^NEW-' : ['new'] };
  expect(journeyOwnershipChanges(block(undefined), next).map((c) => c.key)).toEqual([key]);
});
it('CUJ-04 same diff cannot grant itself producer trust and add a journey', () => {
  const base = block(undefined, []);
  const head = block(j());
  const r = repo({}, { version: 1, journeys: head });
  vi.mocked(showAt).mockReturnValue(JSON.stringify({ version: 1, journeys: base }));
  const findings = checkJourneys(r.ctx('main')).violations.filter((v) => v.rule === 'JRN-3');
  expect(findings.map((v) => v.message.split(' in this')[0])).toEqual(['journeys.trustedProducers changed', 'journey CUJ-CONTACT added']);
  expect(checkJourneys(r.ctx('main', ['guardrails:config-approved'])).violations.filter((v) => v.rule === 'JRN-3')).toEqual([]);
});
it('CUJ-04 removal of the last protected journey still checks ownership', () => {
  const r = repo({}, { version: 1, journeys: block(undefined) });
  vi.mocked(showAt).mockReturnValue(JSON.stringify({ version: 1, journeys: block(j({ provenance: 'client-provided' })) }));
  expect(checkJourneys(r.ctx('main')).violations.filter((v) => v.rule === 'JRN-3').map((v) => v.message.split(' in this')[0])).toEqual(['journey CUJ-CONTACT removed']);
});
