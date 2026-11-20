import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmdirSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { checkFailsOnBase, unlinkDirLink } from '../src/checks/failbase.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const temps: { dir: string; links: string[] }[] = [];

afterEach(() => {
  while (temps.length) {
    const entry = temps.pop() as { dir: string; links: string[] };
    for (const link of entry.links) {
      try {
        unlinkSync(link);
      } catch {
        try {
          rmdirSync(link);
        } catch {
          void 0;
        }
      }
    }
    rmSync(entry.dir, { recursive: true, force: true });
  }
});

function makeLinkedTarget(linkType: 'dir' | 'junction') {
  const root = mkdtempSync(join(tmpdir(), 'guardrails-link-'));
  const real = join(root, 'real');
  const parent = join(root, 'worktree');
  const link = join(parent, 'node_modules');
  const canary = join(real, 'KEEP_ME');
  mkdirSync(real, { recursive: true });
  mkdirSync(parent, { recursive: true });
  writeFileSync(canary, 'keep-me');
  symlinkSync(real, link, linkType);
  temps.push({ dir: root, links: [link] });
  return { root, real, parent, link, canary };
}

describe('FAILBASE-01 base worktree node_modules link cleanup', () => {
  it('FAILBASE-01 unlinkDirLink removes a directory symlink without touching the target', () => {
    const { real, parent, link, canary } = makeLinkedTarget('dir');
    unlinkDirLink(link);
    expect(existsSync(link)).toBe(false);
    expect(() => lstatSync(link)).toThrow();
    rmSync(parent, { recursive: true, force: true });
    expect(existsSync(real)).toBe(true);
    expect(existsSync(canary)).toBe(true);
    expect(readFileSync(canary, 'utf8')).toBe('keep-me');
  });

  it('FAILBASE-01 unlinkDirLink removes a Windows junction without touching the target', () => {
    const { real, parent, link, canary } = makeLinkedTarget('junction');
    unlinkDirLink(link);
    expect(existsSync(link)).toBe(false);
    expect(() => lstatSync(link)).toThrow();
    rmSync(parent, { recursive: true, force: true });
    expect(existsSync(real)).toBe(true);
    expect(existsSync(canary)).toBe(true);
    expect(readFileSync(canary, 'utf8')).toBe('keep-me');
  });

  it('FAILBASE-01 unlinkDirLink is a no-op on missing paths, real directories, and real files', () => {
    const root = mkdtempSync(join(tmpdir(), 'guardrails-safe-'));
    temps.push({ dir: root, links: [] });
    const missing = join(root, 'does-not-exist');
    const realDir = join(root, 'real-dir');
    const canary = join(realDir, 'KEEP_ME');
    const realFile = join(root, 'real-file.txt');
    mkdirSync(realDir, { recursive: true });
    writeFileSync(canary, 'still-here');
    writeFileSync(realFile, 'plain-file');

    expect(() => unlinkDirLink(missing)).not.toThrow();
    expect(existsSync(missing)).toBe(false);

    unlinkDirLink(realDir);
    expect(existsSync(realDir)).toBe(true);
    expect(lstatSync(realDir).isDirectory()).toBe(true);
    expect(readFileSync(canary, 'utf8')).toBe('still-here');

    unlinkDirLink(realFile);
    expect(existsSync(realFile)).toBe(true);
    expect(readFileSync(realFile, 'utf8')).toBe('plain-file');
  });

  it('FAILBASE-01 checkFailsOnBase preserves the repo node_modules directory during worktree cleanup', () => {
    const cfg = { ...BASE_CONFIG, tests: { command: 'node stub.mjs' } };
    const r = repo(
      {
        '.gitignore': 'node_modules\n',
        'stub.mjs': 'export {};\n',
        'src/core/add.ts': 'export const add = (a: number, b: number) => a - b;\n',
      },
      cfg,
    ).commit();
    const nm = join(r.dir, 'node_modules');
    const canary = join(nm, 'KEEP_ME');
    mkdirSync(nm, { recursive: true });
    writeFileSync(canary, 'preserve-me');

    r.write({
      'src/core/add.ts': 'export const add = (a: number, b: number) => a + b;\n',
      'src/core/add.test.ts': "import { expect, it } from 'vitest';\nimport { add } from './add';\nit('CORE-01 adds', () => { expect(add(2, 2)).toBe(4); });\n",
    });

    const out = checkFailsOnBase(r.ctx('main'));
    expect(out.metrics?.[0]?.name).toBe('failsOnBase');
    expect(out.metrics?.[0]?.value).toBe('0/1');
    expect(existsSync(nm)).toBe(true);
    expect(existsSync(canary)).toBe(true);
    expect(readFileSync(canary, 'utf8')).toBe('preserve-me');
  });
});
