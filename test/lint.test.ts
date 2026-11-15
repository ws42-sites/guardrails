import { describe, expect, it } from 'vitest';
import { checkLint } from '../src/checks/lint.js';
import { repo } from './helpers/repo.js';

const lint = async (files: Record<string, string>) =>
  (await checkLint(repo(files).ctx())).violations.map((v) => `${v.file}:${v.line} ${v.message.split(/[.;:]/)[0]}`);

describe('BAN-01 focused, skipped, empty and snapshot tests are banned', () => {
  it('flags .only, .skip, xit, todo, each([]), class assertions, fake servers and Stryker comments', async () => {
    const src = [
      "it.only('CORE-01 a', () => { expect(1).toBe(1) });",
      "it.skip('CORE-01 b', () => { expect(1).toBe(1) });",
      "xit('CORE-01 c', () => { expect(1).toBe(1) });",
      "it.todo('CORE-01 d');",
      "it.each([])('CORE-01 e', () => { expect(1).toBe(1) });",
      "it('CORE-01 f', () => { expect(el).toHaveClass('x'); http.createServer(); });",
      'for (let i = 0; i < 100; i++) { it(`CORE-01 g${i}`, () => { expect(i).toBe(i) }) }',
    ].join('\n');
    expect(await lint({ 'src/core/a.test.ts': src, 'src/core/a.ts': '// Stryker disable all\nexport const a = 1;\n' })).toEqual([
      'src/core/a.test.ts:1 Focused tests are not allowed',
      'src/core/a.test.ts:2 Disabled test (vitest/no-disabled-tests)',
      'src/core/a.test.ts:3 Disabled test (vitest/no-disabled-tests)',
      'src/core/a.test.ts:4 test',
      'src/core/a.test.ts:5 each([]) with an empty table runs nothing',
      'src/core/a.test.ts:6 Class-name/style assertions are banned',
      'src/core/a.test.ts:6 Hand-rolled fake servers are banned',
      'src/core/a.test.ts:7 Loop over 100 iterations in a test',
      'src/core/a.ts:1 "Stryker disable" comments are banned',
    ]);
  });

  it('bans snapshots only in files with T1 spec IDs', async () => {
    const snap = (id: string) => `it('${id} s', () => { expect(render()).toMatchSnapshot() });`;
    expect(await lint({ 'src/core/t1.test.ts': snap('CORE-01'), 'src/lib/t2.test.ts': snap('LIB-01') })).toEqual([
      'src/core/t1.test.ts:1 Snapshots are banned in T1 tests',
    ]);
  });
});

describe('BAN-02 real clocks and randomness are banned unless faked or seeded', () => {
  it.each([
    ['const t = Date.now();', ['Real clock in a test']],
    ['const t = new Date();', ['Real clock in a test']],
    ['const t = performance.now();', ['Real clock in a test']],
    ['const id = crypto.randomUUID(); const r = Math.random();', ['Randomness in a test', 'Randomness in a test']],
    ["vi.useFakeTimers(); vi.setSystemTime(new Date('2026-01-01')); const t = Date.now();", []],
    ['const seed = 42; const r = Math.random();', []],
    ["const t = new Date('2026-01-01');", []],
  ])('%s', async (body, expected) => {
    const out = await lint({ 'src/lib/a.test.ts': `it('LIB-01 x', () => { ${body} expect(1).toBe(1) });` });
    expect(out.map((l) => l.replace(/^\S+ /, ''))).toEqual(expected);
  });
});
