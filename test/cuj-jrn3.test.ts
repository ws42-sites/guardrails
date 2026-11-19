import { j, block, ownershipCases } from './fixtures/cuj/cases.js';
import { expect, it, vi } from 'vitest';
import { checkJourneys, journeyOwnershipChanges } from '../src/checks/journeys.js';
import { showAt } from '../src/util.js';
import { repo } from './helpers/repo.js';

vi.mock('../src/util.js', async (original) => ({ ...await original<typeof import('../src/util.js')>(), mergeBase: () => 'base', showAt: vi.fn(), diffAgainst: () => ({}) }));
it.each(ownershipCases)('CUJ-04 ownership table: %s', (_name, before, after, blocked) => {
  const changes = journeyOwnershipChanges(block(before), block(after));
  expect(changes.length).toBe(blocked ? 1 : 0);
  if (blocked) expect(changes[0].message).toContain('journey CUJ-CONTACT');
});
it.each(['trustedProducers', 'idPattern', 'e2eGlobs'] as const)('CUJ-04 policy change %s requires approval even with no items', (key) => {
  const next = { ...block(undefined), [key]: key === 'idPattern' ? '^NEW-' : ['new'] };
  expect(journeyOwnershipChanges(block(undefined), next).map((c) => c.key)).toEqual([key]);
});
it.each([
  ['self-granted trust', block(undefined, []), block(j()), ['journeys.trustedProducers changed', 'journey CUJ-CONTACT added']],
  ['last protected removal', block(j({ provenance: 'client-provided' })), block(undefined), ['journey CUJ-CONTACT removed']],
])('CUJ-04 %s requires approval', (_name, base, head, expected) => {
  const r = repo({}, { version: 1, journeys: head });
  vi.mocked(showAt).mockReturnValue(JSON.stringify({ version: 1, journeys: base }));
  const findings = checkJourneys(r.ctx('main')).violations.filter((v) => v.rule === 'JRN-3');
  expect(findings.map((v) => v.message.split(' in this')[0])).toEqual(expected);
  expect(checkJourneys(r.ctx('main', ['guardrails:config-approved'])).violations.filter((v) => v.rule === 'JRN-3')).toEqual([]);
});
