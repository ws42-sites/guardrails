import { expect, it } from 'vitest';
import { resolveJourney, resolveTokens, UnresolvedTokensError } from '../src/cuj-tokens.js';

it('CUJ-02 resolves repeated dotted paths and preserves false and zero', () => {
  expect(resolveTokens('{{client.phone}} / {{client.phone}} / {{zero}} / {{flag}}', { client: { phone: '555-0100' }, zero: 0, flag: false })).toBe('555-0100 / 555-0100 / 0 / false');
});
it.each([undefined, null, '', '  ', {}, [], '{{other}}'])('CUJ-02 reports every unresolved or empty token for %j', (empty) => {
  try { resolveTokens('{{client.phone}} {{missing}} {{client.phone}}', { client: { phone: empty } }); }
  catch (e) {
    expect(e).toBeInstanceOf(UnresolvedTokensError);
    expect((e as UnresolvedTokensError).tokens).toEqual(['client.phone', 'missing']);
    return;
  }
  expect.fail('unresolved tokens must throw');
});
it('CUJ-02 rejects prototype traversal and malformed raw tokens', () => {
  expect(() => resolveTokens('{{toString}}', {})).toThrow(UnresolvedTokensError);
  expect(() => resolveTokens('{{client.phone', {})).toThrow(UnresolvedTokensError);
});
it('CUJ-02 copies and resolves all prose and nested run values while retaining metadata', () => {
  const journey = { title: 'Call {{client.phone}}', persona: '{{who}}', goal: '{{who}} calls', success: '{{who}} reached', steps: ['Call {{client.phone}}'], source: { by: 'director' }, run: [{ fill: { label: '{{who}}', value: '{{client.phone}}' } }, { expectHref: { selector: '{{selector}}', value: 'tel:{{client.phone}}' } }] };
  expect(resolveJourney(journey, { client: { phone: '555' }, who: 'Customer', selector: 'a' })).toEqual({ title: 'Call 555', persona: 'Customer', goal: 'Customer calls', success: 'Customer reached', steps: ['Call 555'], source: { by: 'director' }, run: [{ fill: { label: 'Customer', value: '555' } }, { expectHref: { selector: 'a', value: 'tel:555' } }] });
  expect(journey.title).toBe('Call {{client.phone}}');
});
it('CUJ-02 aggregates unresolved tokens across every journey field', () => {
  try { resolveJourney({ title: '{{title}}', goal: '{{goal}}', run: [{ expectText: '{{result}}' }] }, {}); }
  catch (e) { expect((e as UnresolvedTokensError).tokens).toEqual(['title', 'goal', 'result']); return; }
  expect.fail('expected all missing tokens');
});
