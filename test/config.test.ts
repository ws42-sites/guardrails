import { describe, expect, it } from 'vitest';
import { ConfigError, parseConfig } from '../src/config.js';
import { compareConfigs } from '../src/checks/tests.js';
import { BASE_CONFIG } from './helpers/repo.js';

describe('CFG-01 config schema rejects unknown keys and bad values', () => {
  it.each([
    [{ ...BASE_CONFIG, tierz: {} }, /Unrecognized key\(s\) in object: 'tierz'/],
    [{ ...BASE_CONFIG, tests: { maxFileLine: 10 } }, /tests: Unrecognized key\(s\) in object: 'maxFileLine'/],
    [{ ...BASE_CONFIG, rules: { 'TEST-99': 'off' } }, /rules: unknown rule id TEST-99/],
    [{ ...BASE_CONFIG, specs: { items: { 'bad-1': { tier: 'T1', behaviour: 'xyz' } } } }, /specs.items.bad-1: does not match specs.idPattern/],
    [{ ...BASE_CONFIG, version: 2 }, /version: Invalid literal value, expected 1/],
  ])('rejects %#', (raw, message) => {
    expect(() => parseConfig(raw)).toThrow(ConfigError);
    expect(() => parseConfig(raw)).toThrow(message);
  });

  it('applies documented defaults', () => {
    const c = parseConfig(BASE_CONFIG);
    expect(c.tests.budget).toEqual({ T1: 1, T2: 0.6, T3: 0.4 });
    expect(c.tests.maxFileLines).toBe(300);
    expect(c.specs.maxTestsPerId).toBe(8);
    expect(c.tests.testsRemovedLabel).toBe('guardrails:tests-removed-approved');
    expect(c.docs.maxLines).toEqual({ default: 250, 'AGENTS.md': 150 });
  });
});

describe('DIFF-03 loosening the config counts as weakening', () => {
  it.each([
    [{ mutation: { break: 70 } }, { mutation: { break: 60 } }, ['mutation.break: 70 -> 60']],
    [{ tests: { maxFileLines: 300 } }, { tests: { maxFileLines: 400 } }, ['tests.maxFileLines: 300 -> 400']],
    [{ tests: { budget: { T1: 1 } } }, { tests: { budget: { T1: 2 } } }, ['tests.budget.T1: 1 -> 2']],
    [{ rules: { 'TEST-5': 'block' } }, { rules: { 'TEST-5': 'warn' } }, ['rules.TEST-5: block -> warn']],
    [{}, { rules: { 'TEST-7': 'off' } }, ['rules.TEST-7: set to off']],
    [{ deps: { allow: [] } }, { deps: { allow: ['left-pad'] } }, ['deps.allow: added "left-pad"']],
    [{ tiers: { T1: ['a/**', 'b/**'] } }, { tiers: { T1: ['a/**'] } }, ['tiers.T1: removed "b/**"']],
    [{ specs: { items: { 'A-01': { tier: 'T1' } } } }, { specs: { items: { 'A-01': { tier: 'T2' } } } }, ['specs.items.A-01.tier: T1 -> T2']],
  ])('flags change %#', (before, after, expected) => {
    expect(compareConfigs(before, after)).toEqual(expected);
  });

  it('allows tightening and retiring a spec', () => {
    expect(compareConfigs(
      { mutation: { break: 60 }, rules: { 'TEST-8': 'warn' }, specs: { items: { 'A-01': { tier: 'T1' } } } },
      { mutation: { break: 80 }, rules: { 'TEST-8': 'block' }, specs: { items: {} } },
    )).toEqual([]);
  });
});
