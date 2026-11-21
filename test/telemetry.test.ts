import { afterEach, expect, it, vi } from 'vitest';
import { existsSync, readFileSync, mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { send, telemetryStatus } from '../src/telemetry.js';
import { loadConfig, parseConfig } from '../src/config.js';
import { tmpdir } from 'node:os';

const roots: string[] = [];
function repo(files: Record<string, string> = {}, config: object = { version: 1 }) {
  const dir = mkdtempSync(join(tmpdir(), 'guardrails-telemetry-'));
  roots.push(dir);
  const result = {
    dir,
    write(entries: Record<string, string>) {
      for (const [file, text] of Object.entries(entries)) {
        mkdirSync(dirname(join(dir, file)), { recursive: true });
        writeFileSync(join(dir, file), text);
      }
      return result;
    },
    remove(file: string) { rmSync(join(dir, file)); return result; },
  };
  return result.write({ 'guardrails.config.json': JSON.stringify(config), ...files });
}
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }); });

const id = '12345678-1234-4234-8234-123456789abc';
const input = { command: 'check' as const, counts: { block: 2, warn: 3 }, durationMs: 123 };
const enabled = parseConfig({ version: 1, telemetry: { enabled: true, endpoint: 'https://example.invalid/events', timeoutMs: 25 } });
const identity = { env: {}, uuid: () => id, version: '0.7.0', node: '22.12.1', os: 'linux' as const };
const expected = { v: 1, event: 'check', version: '0.7.0', node: 22, os: 'linux', command: 'check', counts: { block: 2, warn: 3 }, durationMs: 123, installId: id };
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); vi.unstubAllEnvs(); });

it.each([
  [false, undefined, {}, 'telemetry.enabled is false'],
  [true, undefined, {}, 'telemetry.endpoint is missing'],
  [true, 'https://example.invalid', { GUARDRAILS_TELEMETRY: '0' }, 'GUARDRAILS_TELEMETRY=0'],
  [true, 'https://example.invalid', { CI: 'true' }, 'CI requires GUARDRAILS_TELEMETRY=1'],
  [true, 'https://example.invalid', { CI: '1', GUARDRAILS_TELEMETRY: 'yes' }, 'CI requires GUARDRAILS_TELEMETRY=1'],
])('TELEM-01 disables sending for %j %j %j', async (optIn, endpoint, env, reason) => {
  const r = repo();
  const fetch = vi.fn<typeof globalThis.fetch>();
  const config = parseConfig({ version: 1, telemetry: { enabled: optIn, endpoint } });
  const deps = { ...identity, cwd: r.dir, config, env, fetch };
  expect(await send('check', input, deps)).toBe(undefined);
  expect(fetch.mock.calls).toEqual([]);
  expect(existsSync(join(r.dir, 'node_modules/.cache/guardrails/install-id'))).toBe(false);
  expect(telemetryStatus('check', input, deps)).toEqual({ enabled: false, reason, payload: expected });
});

it.each([{}, { CI: 'false' }, { CI: '0' }, { CI: '' }, { CI: 'true', GUARDRAILS_TELEMETRY: '1' }])('TELEM-01 sends only anonymous fields for %j', async (env) => {
  vi.useFakeTimers();
  const r = repo();
  const fetch = vi.fn<typeof globalThis.fetch>().mockResolvedValue(new Response('', { status: 200 }));
  const deps = { ...identity, cwd: r.dir, config: enabled, env, fetch };
  await send('check', { ...input, counts: { ...input.counts, known: 99 }, hostname: 'private', path: r.dir } as typeof input, deps);
  const [url, request] = fetch.mock.calls[0];
  expect(fetch.mock.calls.length).toBe(1);
  expect(url).toBe('https://example.invalid/events');
  expect({ ...request, signal: request?.signal instanceof AbortSignal }).toEqual({ method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(expected), signal: true });
  expect(request?.signal?.aborted).toBe(false);
  expect(vi.getTimerCount()).toBe(0);
  expect(telemetryStatus('check', input, deps)).toEqual({ enabled: true, reason: 'opted in', payload: expected });
});

it('TELEM-01 persists the install identifier and honours Markdown opt-out', async () => {
  const r = repo({ 'guardrails.md': '---\ntelemetry.enabled: false\n---' }, { version: 1, telemetry: { endpoint: 'https://example.invalid/events' } });
  const deps = { ...identity, cwd: r.dir, config: enabled };
  expect(telemetryStatus('check', input, deps).payload).toEqual(expected);
  expect(readFileSync(join(r.dir, 'node_modules/.cache/guardrails/install-id'), 'utf8')).toBe(id);
  expect(telemetryStatus('check', input, { ...deps, uuid: () => { throw new Error('must reuse'); } }).payload).toEqual(expected);
  const fetch = vi.fn<typeof globalThis.fetch>();
  await send('check', input, { ...identity, cwd: r.dir, fetch });
  expect(fetch.mock.calls).toEqual([]);
  expect(loadConfig(r.dir).telemetry.enabled).toBe(false);
});

it.each(['init', 'report'] as const)('TELEM-01 preserves the %s event and runtime metadata', (command) => {
  const r = repo({}, { version: 1 });
  vi.stubEnv('CI', '');
  const status = telemetryStatus(command, { ...input, command }, { cwd: r.dir, uuid: () => id });
  expect(status).toEqual({ enabled: false, reason: 'telemetry.enabled is false', payload: { ...expected, event: command, command, version: (JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as { version: string }).version, node: Number(process.versions.node.split('.')[0]), os: process.platform } });
});

it.each(['reject', 'throw', 'http'] as const)('TELEM-02 swallows %s failures and clears the timeout', async (mode) => {
  vi.useFakeTimers();
  const r = repo();
  const fetch = vi.fn<typeof globalThis.fetch>(() => {
    if (mode === 'throw') throw new Error('offline');
    return mode === 'reject' ? Promise.reject(new Error('offline')) : Promise.resolve(new Response('', { status: 500 }));
  });
  expect(await send('check', input, { ...identity, cwd: r.dir, config: enabled, fetch })).toBe(undefined);
  expect(fetch.mock.calls.length).toBe(1);
  expect(vi.getTimerCount()).toBe(0);
});

it('TELEM-02 aborts at the configured deadline even when fetch ignores abort', async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-01-01T00:00:00Z'));
  const r = repo();
  const fetch = vi.fn<typeof globalThis.fetch>(() => new Promise(() => {}));
  let finished = false;
  const sending = send('check', input, { ...identity, cwd: r.dir, config: enabled, fetch }).then(() => { finished = true; });
  expect(finished).toBe(false);
  await vi.advanceTimersByTimeAsync(24);
  expect(fetch.mock.calls[0][1]?.signal?.aborted).toBe(false);
  expect(finished).toBe(false);
  await vi.advanceTimersByTimeAsync(1);
  await sending;
  expect(fetch.mock.calls[0][1]?.signal?.aborted).toBe(true);
  expect(finished).toBe(true);
  expect(vi.getTimerCount()).toBe(0);
});

it.each(['config', 'uuid', 'storage', 'corrupt'] as const)('TELEM-02 ignores %s errors before networking', async (failure) => {
  const r = repo();
  if (failure === 'config') r.remove('guardrails.config.json');
  if (failure === 'storage') r.write({ node_modules: 'not a directory' });
  if (failure === 'corrupt') r.write({ 'node_modules/.cache/guardrails/install-id': 'private@example.invalid' });
  const fetch = vi.fn<typeof globalThis.fetch>();
  const deps = { ...identity, cwd: r.dir, fetch, ...(failure === 'config' ? {} : { config: enabled }), ...(failure === 'uuid' ? { uuid: () => { throw new Error('unavailable'); } } : {}) };
  expect(await send('check', input, deps)).toBe(undefined);
  expect(fetch.mock.calls).toEqual([]);
});

it('TELEM-02 supports the default fetch without making a network request', async () => {
  const r = repo({}, { version: 1, telemetry: enabled.telemetry });
  const fetch = vi.fn<typeof globalThis.fetch>().mockResolvedValue(new Response('ok'));
  vi.stubGlobal('fetch', fetch);
  vi.stubEnv('GUARDRAILS_TELEMETRY', '1');
  expect(await send('check', input, { cwd: r.dir, uuid: () => id })).toBe(undefined);
  expect(JSON.parse(String(fetch.mock.calls[0][1]?.body))).toEqual({ ...expected, version: (JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as { version: string }).version, node: Number(process.versions.node.split('.')[0]), os: process.platform });
});

