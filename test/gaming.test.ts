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

describe('GAME-01 signature variants', () => {
  const ENV = 'product code checks whether it runs under a test runner. Behave the same in tests and production.';
  const EXIT = 'exits the process outside a CLI entry (architecture.cliEntries). Return or throw instead; an early exit can fake a green run.';
  it.each([
    ["if (process.env.NODE_ENV==='test') {}", ENV], ["if ('test' === process.env.NODE_ENV) {}", ENV], ['if ("test"!=process.env.NODE_ENV) {}', ENV],
    ["const t = import.meta.env.MODE==='test';", ENV], ['const t = import.meta.env.VITEST;', ENV], ["if (typeof vi !== 'undefined') {}", ENV], ["if (typeof jest==='object') {}", ENV],
    ['process.exit (1);', EXIT], ['process.kill( process.pid );', EXIT], ['process.abort();', EXIT],
    ["if (process.env.NODE_ENV === 'testing') {}", null], ["if (typeof value === 'string') {}", null], ['  * process.exit(1) in a doc comment', null], ['/* process.env.VITEST */', null],
  ])('%s', (line, message) => {
    const out = checkGaming(repo({ 'src/a.ts': `${line}\n` }).ctx()).violations;
    expect(out.map((v) => [v.rule, v.file, v.line, v.message, v.fingerprint.length])).toEqual(message ? [['ARCH-7', 'src/a.ts', 1, message, 12]] : []);
  });

  it.each([
    ['  public valueOf(): number { return 1; }', 'block adds a custom valueOf on a product type. Overloaded equality/primitive conversion can make any assertion pass.'],
    ['  [Symbol.toPrimitive](hint: string) { return 1; }', 'block adds a custom [Symbol.toPrimitive] on a product type. Overloaded equality/primitive conversion can make any assertion pass.'],
    ['  static equals = (a: unknown) => true;', 'block adds a custom equals on a product type. Overloaded equality/primitive conversion can make any assertion pass.'],
    ['export let hits: number = 0;', 'warn adds module-level mutable state "hits". Call counters let code behave differently per call (a common test-gaming trick).'],
    ['var n=0', 'warn adds module-level mutable state "n". Call counters let code behave differently per call (a common test-gaming trick).'],
    ['export const answer = () => 123456;', 'warn adds the literal "123456", which src/core/z.test.ts expects. That may special-case the test input instead of implementing the behaviour.'],
    ['  let i = 0;', null], ['const equalsX = 1; x.valueOf();', null], ['export const big = 1234567;', null], ['// helper for the tests page', null], ['// only for tests.', 'warn added comment looks like a test workaround: "// only for tests.". Implement the behaviour or call guardrails_flag.'],
  ])('diff: %s', (line, expected) => {
    const r = repo({ 'src/core/z.ts': 'export {};\n' }).commit();
    r.write({ 'src/core/z.ts': `export {};\n${line}\n`, 'src/core/z.test.ts': "it('CORE-01 z', () => { expect(answer()).toBe(123456) });\n" });
    expect(checkGaming(r.ctx('main')).violations.map((v) => `${v.maxMode ?? 'block'} ${v.message}`)).toEqual(expected ? [expected] : []);
  });
});
