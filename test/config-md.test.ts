import { expect, it } from 'vitest';
import { configFromSources, mergeConfig, parseConfigMarkdown } from '../src/config-md.js';
import { ConfigError, loadConfig, parseConfig } from '../src/config.js';
import { guard, projectRoot } from '../src/hook.js';
import { checkSetup } from '../src/checks/setup.js';
import { checkTests } from '../src/checks/tests.js';
import { join } from 'node:path';
import { repo } from './helpers/repo.js';

it.each(['\n', '\r\n'])('CFGMD-01 merges document order, front matter and JSON with %j', (newline) => {
  const md = ['---', '# ignored', '', 'review.enabled: true', 'baseBranch: "release"', 'mutation.break: 85', 'sourceRoots: ["app"]', 'review.stub: {"failOn":["bad"]}', '---', '# Prose', '```guardrails', '{"review":{"enabled":false,"timeoutSeconds":12},"mutation":{"break":80},"sourceRoots":["lib"]}', '```', '```json guardrails', '{"review":{"timeoutSeconds":20},"mutation":{"break":81}}', '```'].join(newline);
  expect(configFromSources(md, '{"review":{"enabled":false}}')).toEqual(parseConfig({ version: 1, baseBranch: 'release', review: { enabled: false, timeoutSeconds: 20, stub: { failOn: ['bad'] } }, mutation: { break: 85 }, sourceRoots: ['app'] }));
});

it.each([
  ['', {}], ['Plain prose', {}],
  ['```js\n```guardrails\n{"version":2}\n```\n', {}],
  ['~~~guardrails\n{"mutation":{"break":90}}\n~~~~', { mutation: { break: 90 } }],
  ['---\nbaseBranch: release\ntelemetry.enabled: false\nmutation.break: 1.2e1\n---', { baseBranch: 'release', telemetry: { enabled: false }, mutation: { break: 12 } }],
  ['```guardrails\n{"sourceRoots":["a","b"],"review":{"stub":{"failOn":["a"]}}}\n```\n```guardrails\n{"sourceRoots":["c"],"review":{"stub":{"failOn":["b"]}}}\n```', { sourceRoots: ['c'], review: { stub: { failOn: ['b'] } } }],
])('CFGMD-01 parses supported content %j', (md, expected) => {
  expect(parseConfigMarkdown(md)).toEqual(expected);
});

it('CFGMD-01 replaces scalars and arrays without mutating inputs or object prototypes', () => {
  const lower = { a: { x: 1 }, b: [1, 2], c: 3, d: null };
  expect(mergeConfig(lower, { a: 2, b: [3], c: { y: 4 }, d: { x: 5 } })).toEqual({ a: 2, b: [3], c: { y: 4 }, d: { x: 5 } });
  expect(lower).toEqual({ a: { x: 1 }, b: [1, 2], c: 3, d: null });
  const result = mergeConfig({}, JSON.parse('{"__proto__":{"polluted":true},"constructor":{"x":1}}'));
  expect(Object.getPrototypeOf(result)).toBe(Object.prototype);
  expect(Object.hasOwn(result, '__proto__')).toBe(true);
  expect(result.constructor).toEqual({ x: 1 });
});

it.each(['markdown', 'json', 'both'])('CFGMD-01 loads %s configuration and protects Markdown', (kind) => {
  const r = repo();
  if (kind !== 'json') r.write({ 'guardrails.md': '---\nbaseBranch: develop\n---' });
  if (kind === 'markdown') r.remove('guardrails.config.json');
  expect(loadConfig(r.dir).baseBranch).toBe(kind === 'json' ? 'main' : 'develop');
  expect(loadConfig(r.dir).lock.paths.includes('guardrails.md')).toBe(true);
  expect(configFromSources(null, null).version).toBe(1);
});

it.each([
  ['80', '70', null, ['guardrails.md']],
  ['80', '90', null, []],
  ['80', '70', 95, []],
  ['80', null, null, ['guardrails.md']],
])('CFGMD-01 detects effective weakening from %s to %s', (old, next, override, expected) => {
  const json = { version: 1, ...(override === null ? {} : { mutation: { break: override } }) };
  const r = repo({ 'guardrails.md': `---\nmutation.break: ${old}\n---` }, json).commit();
  if (next === null) r.remove('guardrails.md');
  else r.write({ 'guardrails.md': `---\nmutation.break: ${next}\n---` });
  r.commit();
  const ctx = { ...r.ctx('HEAD~1'), config: loadConfig(r.dir) };
  expect(checkTests(ctx).violations.filter((v) => v.rule === 'TEST-7').map((v) => v.file)).toEqual(expected);
  expect(checkTests({ ...ctx, labels: ['guardrails:config-approved'] }).violations.filter((v) => v.rule === 'TEST-7')).toEqual([]);
});

it('CFGMD-01 checks ownership of Markdown-only configuration', () => {
  const r = repo({ 'guardrails.md': '', '.github/CODEOWNERS': '/guardrails.md @o\n/.github/ @o\n/.guardrails/ @o\n' }).remove('guardrails.config.json');
  expect(configFromSources('', null)).toEqual(loadConfig(r.dir));
  const ctx = { cwd: r.dir, config: loadConfig(r.dir), base: null, labels: [], options: { clockShift: false, mutation: false, knip: false } };
  expect(checkSetup(ctx).violations).toEqual([]);
});

it.each([
  ['---\nno colon\n---', 2, 'expected dotted.key: value'],
  ['---\na..b: true\n---', 2, 'expected dotted.key: value'],
  ['---\nreview.enabled: true', 1, 'unclosed front matter'],
  ['---\nreview.enabled:\n---', 2, 'expected a boolean, number, string or inline JSON'],
  ['---\nbaseBranch: "bad\n---', 2, 'invalid JSON value'],
  ["---\nbaseBranch: 'bad'\n---", 2, 'expected a boolean, number, string or inline JSON'],
  ['---\ndeps.allow: []\n---', 2, 'deps belongs in guardrails.config.json'],
  ['---\njourneys: {}\n---', 2, 'journeys belongs in guardrails.config.json'],
  ['Prose\n```guardrails\n{bad}\n```', 2, 'invalid JSON block'],
  ['```guardrails\n{}', 1, 'unclosed guardrails fence'],
  ['```guardrails\n[]\n```', 1, 'guardrails block must be a JSON object'],
  ['```guardrails\nnull\n```', 1, 'guardrails block must be a JSON object'],
  ['```guardrails\n{"deps":{}}\n```', 1, 'deps belongs in guardrails.config.json'],
  ['```json guardrails\n{"journeys":{}}\n```', 1, 'journeys belongs in guardrails.config.json'],
])('CFGMD-02 rejects malformed or human-owned content %j', (md, line, message) => {
  let error: unknown;
  try { parseConfigMarkdown(md); } catch (caught) { error = caught; }
  expect(error instanceof ConfigError).toBe(true);
  expect((error as Error).message).toBe(`Invalid guardrails.md at line ${line}: ${message}`);
});

it.each(['null', '[]', 'true', '"text"', '{bad}'])('CFGMD-02 rejects invalid JSON source %s', (json) => {
  let error: unknown;
  try { configFromSources(null, json); } catch (caught) { error = caught; }
  expect(error instanceof ConfigError).toBe(true);
  expect((error as Error).message).toBe(json === '{bad}' ? 'guardrails.config.json is not valid JSON' : 'guardrails.config.json must be a JSON object');
});

it('CFGMD-02 locks Markdown and uses its settings from a descendant directory', () => {
  const r = repo({ 'guardrails.md': '---\nlock.paths: ["guardrails.md", "secret.txt"]\n---' }).remove('guardrails.config.json');
  expect(parseConfigMarkdown('')).toEqual({});
  expect(projectRoot({ cwd: join(r.dir, 'src') }, {})).toBe(r.dir);
  for (const path of ['guardrails.md', 'secret.txt']) {
    const result = guard({ cwd: join(r.dir, 'src'), tool_input: { file_path: join(r.dir, path) } }, {});
    expect({ allow: result.allow, path: result.path }).toEqual({ allow: false, path });
  }
});

it('CFGMD-02 reports absent files and validates merged schema', () => {
  const r = repo().remove('guardrails.config.json');
  let message = '';
  try { loadConfig(r.dir); } catch (error) { message = (error as Error).message; }
  expect(message).toBe(`No guardrails.config.json or guardrails.md in ${r.dir}. Run "guardrails init" first.`);
  let invalid = false;
  try { configFromSources('---\nversion: 2\n---', null); } catch (error) { invalid = error instanceof ConfigError; }
  expect(invalid).toBe(true);
});
