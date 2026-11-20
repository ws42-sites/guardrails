import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { marker, parseMarker, verify } from '../src/provenance.js';
import { first, fixture, head, tested } from './helpers/provenance.js';

const failed = { ok: false, violations: [{ rule: 'PROV-1', message: 'Missing, invalid or mismatched provenance or report' }] };

it.each([
  [[tested], tested, true], [[tested], head, false], [[tested, first, head], head, true],
  [[tested, head], head, false], [[tested, head, first], head, false],
  [[tested, first, head, first], head, false], [[tested], '', false],
] as const)('PROV-02 verifies exact commit or exactly two-parent merge %j %s', (parents, target, ok) => {
  const f = fixture([...parents]);
  const expected = ok ? { ok: true, violations: [] } : failed;
  expect(verify(f.value, target)).toEqual(expected);
  expect(verify(join(f.cwd, 'proof.json'), target)).toEqual(expected);
});

it.each(['missing', 'invalid-json', 'invalid-record', 'changed-report', 'missing-report', 'wrong-parent', 'wrong-type'])('PROV-02 fails closed on %s', (kind) => {
  const f = fixture();
  let input: unknown = f.value;
  if (kind === 'missing') input = join(f.cwd, 'missing.json');
  if (kind === 'invalid-json') { writeFileSync(join(f.cwd, 'proof.json'), '{'); input = join(f.cwd, 'proof.json'); }
  if (kind === 'invalid-record') input = {};
  if (kind === 'changed-report') writeFileSync(f.value.report.path, f.body + '\n');
  if (kind === 'missing-report') input = { ...f.value, report: { ...f.value.report, path: join(f.cwd, 'absent') } };
  if (kind === 'wrong-parent') input = { ...f.value, parents: [head] };
  if (kind === 'wrong-type') input = { ...f.value, ok: 'true' };
  expect(verify(input, tested)).toEqual(failed);
});

it('PROV-02 verifies explicitly relocated report bytes', () => {
  const f = fixture();
  const alternate = join(f.cwd, 'alternate.json');
  writeFileSync(alternate, f.body);
  writeFileSync(f.value.report.path, 'changed');
  expect(verify(f.value, tested, alternate)).toEqual({ ok: true, violations: [] });
  writeFileSync(alternate, '{}');
  expect(verify(f.value, tested, alternate)).toEqual(failed);
});

it('PROV-02 round trips a marker inside a markdown summary', () => {
  const f = fixture();
  const result = marker(f.value);
  const payload = JSON.parse(Buffer.from(result.split(' ')[3], 'base64url').toString('utf8'));
  expect(payload.record).toEqual(f.value);
  expect(parseMarker(`Summary\r\n${result}\r\nend`)).toEqual(f.value);
  expect(parseMarker(result + result)).toBe(null);
});

it.each(['foreign', 'version', 'corrupt', 'tampered', 'schema', 'empty', 'noncanonical'])('PROV-02 rejects %s marker', (kind) => {
  const f = fixture();
  let result = marker(f.value);
  if (kind === 'foreign') result = result.replace('guardrails:', 'other:');
  if (kind === 'version') result = result.replace('v1', 'v2');
  if (kind === 'corrupt') result = '<!-- guardrails:provenance v1 abc -->';
  if (kind === 'empty') result = '';
  if (kind === 'noncanonical') result = '<!-- guardrails:provenance v1 Zh -->';
  if (kind === 'tampered' || kind === 'schema') {
    const payload = JSON.parse(Buffer.from(result.split(' ')[3], 'base64url').toString('utf8'));
    if (kind === 'tampered') payload.record.ok = true;
    else payload.record.testedSha = 'bad';
    result = `<!-- guardrails:provenance v1 ${Buffer.from(JSON.stringify(payload)).toString('base64url')} -->`;
  }
  expect(parseMarker(result)).toBe(null);
});

it.each([
  { testedSha: 'a'.repeat(39) }, { testedSha: 'a'.repeat(41) }, { testedSha: 'g'.repeat(40) },
  { testedSha: 'a'.repeat(65) }, { parents: [] }, { parents: ['bad'] }, { prHeadSha: 'bad' },
  { report: { path: '', sha256: 'a'.repeat(64) } }, { report: { path: 'x', sha256: 'a'.repeat(63) } },
  { report: { path: 'x', sha256: 'g'.repeat(64) } }, { report: { path: 'x', sha256: 'a'.repeat(65) } },
  { guardrailsVersion: '' }, { checks: [{ rule: '', mode: 'block', count: 0 }] },
  { checks: [{ rule: 'X', mode: 'invalid', count: 0 }] }, { checks: [{ rule: 'X', mode: 'block', count: -1 }] },
  { checks: [{ rule: 'X', mode: 'block', count: 0.5 }] }, { checks: [{ rule: 'X', mode: 'block', count: 0, extra: true }] },
  { overrides: [{ label: '', by: '', at: '', reason: '' }] }, { overrides: [{ label: 'x', by: 1, at: '', reason: '' }] },
  { overrides: [{ label: 'x', by: '', at: '', reason: '', extra: true }] },
  { generatedAt: 'yesterday' }, { extra: true },
])('PROV-02 rejects malformed binding fields %j', (patch) => {
  const f = fixture();
  expect(verify({ ...f.value, ...patch }, tested)).toEqual(failed);
});
