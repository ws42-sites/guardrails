import { resolve } from 'node:path';
import { vi } from 'vitest';
import { parseConfig } from '../../src/config.js';
import type { CheckContext } from '../../src/types.js';

const fixture = vi.hoisted(() => ({
  files: {} as Record<string, string>, base: {} as Record<string, string>,
  modules: [] as { source: string; dependencies: { resolved: string }[] }[],
  cwd: '',
}));

export { fixture };

vi.mock('node:fs', async (original) => {
  const fs = await original<typeof import('node:fs')>();
  const key = (p: unknown) => String(p).replaceAll('\\', '/').slice(fixture.cwd.length + 1);
  const local = (p: unknown) => String(p).replaceAll('\\', '/').startsWith(fixture.cwd + '/');
  return { ...fs,
    existsSync: (p: unknown) => local(p) ? key(p) in fixture.files : fs.existsSync(p as string),
    readFileSync: (p: unknown, options: never) => local(p) ? fixture.files[key(p)] : fs.readFileSync(p as string, options),
  };
});

vi.mock('../../src/util.js', async (original) => {
  const util = await original<typeof import('../../src/util.js')>();
  return { ...util,
    listFiles: () => Object.keys(fixture.files),
    readText: (_cwd: string, file: string) => {
      if (!(file in fixture.files)) throw new Error(`Missing fixture: ${file}`);
      return fixture.files[file];
    },
    filesAt: () => Object.keys(fixture.base),
    mergeBase: (_cwd: string, ref: string) => {
      if (ref === 'missing') throw new Error('Unknown base');
      return ref;
    },
    showAt: (_cwd: string, _ref: string, file: string) => fixture.base[file] ?? null,
    diffAgainst: () => Object.fromEntries([...new Set([...Object.keys(fixture.base), ...Object.keys(fixture.files)])]
      .filter((f) => fixture.base[f] !== fixture.files[f]).map((f) => [f, {
        status: !(f in fixture.files) ? 'deleted' : f in fixture.base ? 'modified' : 'added',
        added: (fixture.files[f] ?? '').split('\n'), removed: (fixture.base[f] ?? '').split('\n'), addedLines: [1],
      }])),
  };
});

vi.mock('dependency-cruiser', () => ({ cruise: vi.fn(async () => ({ output: { modules: fixture.modules } })) }));

export const title = 'CORE-01 adds';
export const testSource = (value = 1) => `it('${title}', () => { expect(add()).toBe(${value}); });\n`;
export const labels = {
  characterization: 'owner:existing', configApproved: 'owner:config',
  rebaseline: 'owner:baseline', journeysApproved: 'owner:journeys', flagsResolved: 'owner:flags',
};
export const config = {
  version: 1, sourceRoots: ['src'], labels, tests: { testsRemovedLabel: 'owner:removed' },
  specs: { items: { 'CORE-01': { tier: 'T1', behaviour: 'core adds numbers', paths: ['src/core/**'] } } },
};
export function labelRepo(extra: Record<string, string> = {}, cfg: object = config) {
  fixture.cwd = resolve('virtual-label-fixture').replaceAll('\\', '/');
  fixture.modules = [];
  fixture.files = { 'guardrails.config.json': JSON.stringify(cfg), 'src/core/add.ts': 'export const add = () => 1;\n', ...extra };
  fixture.base = { ...fixture.files };
  const r = {
    dir: fixture.cwd,
    write(files: Record<string, string>) { Object.assign(fixture.files, files); return r; },
    remove(...files: string[]) { for (const file of files) delete fixture.files[file]; return r; },
    ctx(base: string | null = null, present: string[] = []): CheckContext {
      return { cwd: fixture.cwd, config: parseConfig(JSON.parse(fixture.files['guardrails.config.json'])), base, labels: present,
        options: { clockShift: false, mutation: false, knip: false } };
    },
  };
  return r;
}

export const flagFile = JSON.stringify({ version: 1, flags: [
  { id: 'F1', specId: 'CORE-01', file: 'src/core/add.ts', reason: 'Conflicting requirement', createdAt: '2026-01-01T00:00:00.000Z' },
] });
