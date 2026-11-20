import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LEGACY_LABELS, hasLabel } from '../src/labels.js';
import { parseConfig } from '../src/config.js';

const NEW = 'guardrails:existing-behaviour-tests-approved';

describe('LABEL-01 approval labels follow guardrails:<function>-approved and keep deprecated aliases', () => {
  let write: { mockRestore(): void; mock: { calls: unknown[][] } };
  beforeEach(() => { write = vi.spyOn(process.stderr, 'write').mockImplementation(() => true) as unknown as typeof write; });
  afterEach(() => { write.mockRestore(); });

  it('defaults every label to the guardrails:*-approved pattern', () => {
    const c = parseConfig({ version: 1 });
    expect([c.labels.journeysApproved, c.labels.configApproved, c.tests.testsRemovedLabel, c.labels.characterization, c.labels.rebaseline, c.labels.flagsResolved]).toEqual([
      'guardrails:journeys-approved', 'guardrails:config-approved', 'guardrails:tests-removed-approved', NEW, 'guardrails:baseline-approved', 'guardrails:flags-approved',
    ]);
  });

  it('accepts the new label without a warning', () => {
    expect(hasLabel([NEW], NEW, LEGACY_LABELS.characterization)).toBe(true);
    expect(write).not.toHaveBeenCalled();
  });

  it('accepts the old name once, warning with the new label', () => {
    expect(hasLabel(['allow-characterization'], NEW, LEGACY_LABELS.characterization)).toBe(true);
    expect(hasLabel(['allow-characterization'], NEW, LEGACY_LABELS.characterization)).toBe(true);
    expect(write).toHaveBeenCalledTimes(1);
    expect(String(write.mock.calls[0][0])).toContain(`Rename it to "${NEW}"`);
  });

  it('rejects an unrelated label', () => {
    expect(hasLabel(['other'], NEW, LEGACY_LABELS.characterization)).toBe(false);
  });
});
