import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { expect, it } from 'vitest';
import { importJourneys, mergeJourneys } from '../src/journeys-import.js';
import { guard } from '../src/hook.js';
import { repo } from './helpers/repo.js';

const j = (fields: object = {}) => ({ id: 'CUJ-CONTACT', title: 'Visitor contacts the business', priority: 'P0', steps: ['Open contact'], provenance: 'inferred', source: { by: 'director' }, run: [{ goto: '/contact' }], ...fields });
const cfg = { version: 1, journeys: { trustedProducers: ['director'], items: {} } };
const cli = resolve('dist/cli.js');
it.each(['inferred', 'client-provided', 'template'])('CUJ-05 imports %s atomically and exact repeats write nothing', (provenance) => {
  const r = repo({ 'input.json': JSON.stringify([j({ provenance })]) }, cfg);
  const file = join(r.dir, 'guardrails.config.json');
  expect(importJourneys(r.dir, join(r.dir, 'input.json'))).toEqual(['CUJ-CONTACT']);
  const saved = readFileSync(file, 'utf8');
  expect(JSON.parse(saved).journeys.items['CUJ-CONTACT']).toEqual({ ...j({ provenance, id: undefined }), approved: false, specs: [], routes: [] });
  expect(importJourneys(r.dir, join(r.dir, 'input.json'))).toEqual([]);
  expect(readFileSync(file, 'utf8')).toBe(saved);
});
it.each([
  [j({ approved: true })], [j({ approvedBy: 'bot' })], [j({ provenance: undefined })],
  [j({ source: undefined })], [j({ source: { by: 'stranger' } })], [j({ run: [{ evaluate: 'bad' }] })],
  [j({ id: 'bad' })], [j(), j()], [j({ specs: ['UNKNOWN-01'] })], {},
])('CUJ-05 refuses invalid imports without writing %j', (input) => {
  const r = repo({ 'input.json': JSON.stringify(input) }, cfg);
  const before = readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8');
  expect(() => importJourneys(r.dir, join(r.dir, 'input.json'))).toThrow();
  expect(readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8')).toBe(before);
});
it.each([{ approved: true, approvedBy: 'Josh' }, { provenance: 'client-provided' }])('CUJ-05 refuses protected overwrite %j including mixed batches', (protectedFields) => {
  const { id, ...existing } = j(protectedFields);
  const raw = { ...cfg, journeys: { ...cfg.journeys, items: { [id]: existing } } };
  const r = repo({ 'input.json': JSON.stringify([j({ id: 'CUJ-NEW' }), j({ goal: 'Changed' })]) }, raw);
  const before = readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8');
  expect(() => importJourneys(r.dir, join(r.dir, 'input.json'))).toThrow('cannot overwrite');
  expect(readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8')).toBe(before);
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
  const r = repo({ 'input.json': JSON.stringify([j()]), 'bad.json': JSON.stringify([j({ approved: true })]) }, cfg);
  const env = { ...process.env, GUARDRAILS_AGENT: 'test-agent' };
  const good = spawnSync(process.execPath, [cli, 'journeys', 'import', 'input.json'], { cwd: r.dir, env, encoding: 'utf8' });
  expect([good.status, good.stderr]).toEqual([0, '']);
  const before = readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8');
  const bad = spawnSync(process.execPath, [cli, 'journeys', 'import', 'bad.json'], { cwd: r.dir, env, encoding: 'utf8' });
  expect(bad.status).toBe(1);
  expect(bad.stderr).toContain('cannot approve');
  expect(readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8')).toBe(before);
  expect(['import input.json', 'approve CUJ-CONTACT', 'draft'].map((arg) => guard({ tool_input: { command: 'npx guardrails journeys ' + arg } }, { CLAUDE_PROJECT_DIR: r.dir }).allow)).toEqual([true, false, false]);
});
