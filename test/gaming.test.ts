import { describe, expect, it } from 'vitest';
import { checkGaming } from '../src/checks/gaming.js';
import { repo } from './helpers/repo.js';

const found = (vs: { file: string; message: string; maxMode?: string }[]) => vs.map((v) => `${v.maxMode ?? 'block'} ${v.file}: ${v.message.split(/[.,(]/)[0]}`);

describe('GAME-01 product code must not detect tests or exit early', () => {
  it.each([
    ["if (process.env.NODE_ENV === 'test') return 3;", 'block src/a.ts: product code checks whether it runs under a test runner'],
    ['const t = process.env.VITEST;', 'block src/a.ts: product code checks whether it runs under a test runner'],
    ["if (process.env['JEST_WORKER_ID']) x();", 'block src/a.ts: product code checks whether it runs under a test runner'],
    ['if (import.meta.vitest) {}', 'block src/a.ts: product code checks whether it runs under a test runner'],
    ['process.exit(0);', 'block src/a.ts: exits the process outside a CLI entry '],
    ['// process.env.VITEST in a comment is fine', null],
    ["const mode = process.env.NODE_ENV ?? 'development';", null],
  ])('%s', (line, expected) => {
    const r = repo({ 'src/a.ts': `export function f() {\n${line}\n}\n`, 'src/cli.ts': 'process.exit(1);\n' });
    expect(found(checkGaming(r.ctx()).violations)).toEqual(expected ? [expected] : []);
  });

  it('bans expect.extend in tests and warns when a test mocks its own module', () => {
    const r = repo({ 'src/core/add.test.ts': "vi.mock('./add');\nexpect.extend({ toBeAnything: () => ({ pass: true }) });\nit('CORE-01 a', () => { expect(1).toBe(1) });\n" });
    expect(found(checkGaming(r.ctx()).violations)).toEqual([
      'block src/core/add.test.ts: expect',
      'warn src/core/add.test.ts: vi',
    ]);
  });
});

describe('GAME-02 diff mode flags equality overloads, counters, shared literals and cheat comments', () => {
  it('blocks added valueOf/equals and warns on counters, literals copied from expectations and cheat comments', () => {
    const r = repo({ 'src/core/add.ts': 'export const add = (a: number, b: number) => a + b;\n' }).commit();
    r.write({
      'src/core/add.ts': [
        'export const add = (a: number, b: number) => a + b;',
        'let calls = 0;',
        "export const label = (s: string) => (s === 'weird-input' ? 'expected-output' : s); // hack to make the test pass",
        'export class Money {',
        '  valueOf() { return 1; }',
        '  equals(o: unknown) { return true; }',
        '}',
      ].join('\n'),
      'src/core/add.test.ts': "it('CORE-01 a', () => { expect(label('weird-input')).toBe('expected-output') });\n",
    });
    expect(found(checkGaming(r.ctx('main')).violations).sort()).toEqual([
      'block src/core/add.ts: adds a custom equals on a product type',
      'block src/core/add.ts: adds a custom valueOf on a product type',
      'warn src/core/add.ts: added comment looks like a test workaround: "export const label = ',
      'warn src/core/add.ts: adds module-level mutable state "calls"',
      'warn src/core/add.ts: adds the literal "expected-output"',
    ]);
  });
});
