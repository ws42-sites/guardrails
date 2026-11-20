import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import { vi } from 'vitest';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { afterEach } from 'vitest';
import * as util from '../../src/util.js';
import { parseConfig } from '../../src/config.js';
import type { ReviewInput, Spawner } from '../../src/review/provider.js';

export const config = (review: object = {}) => parseConfig({ version: 1, review: { enabled: true, onlyWhenRisky: false, ...review } });
export const input: ReviewInput = { diff: { text: '', truncated: false }, testOutput: [{ rule: 'TEST-1' }], files: ['a.ts'] };
const directories: string[] = [];
afterEach(() => { vi.restoreAllMocks(); for (const dir of directories.splice(0)) rmSync(dir, { recursive: true, force: true }); });
export function tree(files: Record<string, string>, diff = '', previous: Record<string, string> = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'review-tree-'));
  directories.push(dir);
  for (const [file, body] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, file)), { recursive: true });
    writeFileSync(join(dir, file), body);
  }
  const calls: string[][] = [];
  vi.spyOn(util, 'listFiles').mockReturnValue(Object.keys(files));
  vi.spyOn(util, 'git').mockImplementation((cwd, args) => {
    if (cwd !== dir) throw new Error('unexpected cwd');
    calls.push(args);
    if (args[0] === 'merge-base') { if (args[1] === 'missing') throw new Error('missing base'); return 'abc\n'; }
    if (args.includes('--name-only')) return [...new Set([...Object.keys(files), ...Object.keys(previous)])].join('\0');
    if (args[0] === 'show') { const file = args[1].slice(4); if (!(file in previous)) throw new Error('missing file'); return previous[file]; }
    return diff;
  });
  return { dir, calls };
}
export function fakeSpawn(action: (child: ChildProcessWithoutNullStreams & { stdout: PassThrough }) => void) {
  const child = Object.assign(new EventEmitter(), { stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: vi.fn(() => true) }) as unknown as ChildProcessWithoutNullStreams & { stdout: PassThrough };
  let sent = '';
  child.stdin.on('data', (chunk: Buffer) => { sent += chunk.toString(); });
  const spawn: Spawner = vi.fn(() => { queueMicrotask(() => action(child)); return child; });
  return { child, spawn, sent: () => JSON.parse(sent) as unknown };
}
