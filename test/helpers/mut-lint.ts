import type { Violation } from '../../src/types.js';
import { fingerprint } from '../../src/util.js';
import { BASE_CONFIG } from './repo.js';

export interface ExpectedLint {
  rule: string;
  file: string;
  line: number | undefined;
  maxMode: string | undefined;
  message: string;
  fingerprint: string;
}

export function normalizeViolations(vs: Violation[]): ExpectedLint[] {
  return vs.map((v) => ({
    rule: v.rule,
    file: v.file,
    line: v.line,
    maxMode: v.maxMode,
    message: v.message,
    fingerprint: v.fingerprint,
  }));
}

export function expectedViolation(
  file: string,
  line: number,
  ruleId: string,
  rawMsg: string,
  trimmedLine: string,
  maxMode?: 'warn',
): ExpectedLint {
  return {
    rule: 'TEST-5',
    file,
    line,
    maxMode,
    message: `${rawMsg} (${ruleId})`,
    fingerprint: fingerprint('TEST-5', file, `${ruleId}:${rawMsg}:${trimmedLine}`),
  };
}

export const LINT_MUT_CONFIG = {
  ...BASE_CONFIG,
  tests: { globs: ['src/**/*.test.*'] },
  specs: {
    items: {
      ...BASE_CONFIG.specs.items,
      'PROP-01': { tier: 'T1', behaviour: 'property invariant holds', properties: true },
    },
  },
} as const;

const STRYKER_COMMENT = ['// ', 'Stryker', ' disable all'].join('');

export function megaFixtureFiles(): Record<string, string> {
  const t1BlocksLines = [
    "import { expect, it } from 'vitest';",
    '/* eslint-disable */',
    'const typedValue: number = 1;',
    'console.log(typedValue);',
    '// eslint-disable-next-line vitest/no-focused-tests',
    "  it.only('CORE-01 focus', () => { expect(typedValue).toBe(1); });  ",
    "it.skip('CORE-01 skip', () => { expect(1).toBe(1); });",
    "xit('CORE-01 xit', () => { expect(1).toBe(1); });",
    "it.todo('CORE-01 todo');",
    "it.each([])('CORE-01 empty', () => { expect(1).toBe(1); });",
    "it('CORE-01 clock', () => { const a = new Date(); const b = Date.now(); const c = performance.now(); const d = process.hrtime(); const e = hrtime.bigint(); expect([a, b, c, d, e]).toHaveLength(5); });",
    "it('CORE-01 rand', () => { const a = Math.random(); const b = crypto.randomUUID(); const c = crypto.getRandomValues(new Uint8Array(1)); expect([a, b, c]).toHaveLength(3); });",
    "it('CORE-01 fc', () => { fc.assert(fc.property(fc.integer(), (n) => n === n), { numRuns: 21 }); expect(1).toBe(1); });",
    STRYKER_COMMENT,
    "it('CORE-01 snap', () => { expect(1).toMatchSnapshot(); expect(2).toMatchInlineSnapshot(); });",
  ];

  const t1WarnsLines = [
    "it('CORE-01 warns', () => {",
    '  const el = <span />;',
    "  expect(el).toHaveClass('a');",
    "  expect(el).toHaveStyle('color: red');",
    '  http.createServer();',
    "  vi.spyOn(Date, 'now');",
    "  vi.spyOn(Response, 'json');",
    "  vi.spyOn(crypto.subtle, 'digest');",
    "  vi.spyOn(Array.prototype, 'map');",
    '  for (let i = 0; i < 21; i++) { expect(i).toBe(i); }',
    '  Array.from({ length: 25 }, (_, i) => i);',
    '  fc.assert(fc.property(fc.integer(), (n) => n === n), { seed: 42, interruptAfterTimeLimit: 1000 });',
    '});',
  ];

  return {
    '.eslintrc.json': JSON.stringify({
      root: true,
      rules: { 'no-console': 'error', 'vitest/no-focused-tests': 'off', 'no-restricted-syntax': 'off' },
    }),
    '.eslintignore': '*\n',
    'src/core/t1-blocks.test.ts': `${t1BlocksLines.join('\n')}\n`,
    'src/core/t1-warns.test.tsx': `${t1WarnsLines.join('\n')}\n`,
    'src/lib/t2-clean.test.js': [
      "import { expect, it } from 'vitest';",
      "it('LIB-01 clean js', () => {",
      '  const node = <div />;',
      "  const fixed = new Date('2026-01-01');",
      '  for (let i = 0; i < 20; i++) { expect(i).toBe(i); }',
      '  Array.from({ length: 20 }, (_, i) => i);',
      '  fc.assert(fc.property(fc.integer(), (n) => n === n), { numRuns: 20 });',
      '  expect([node, fixed]).toMatchSnapshot();',
      '});',
    ].join('\n'),
    'src/lib/t2-jsx.test.jsx': "it('LIB-01 jsx', () => { const node = <div />; expect(node).toMatchSnapshot(); expect(node).toHaveClass('x'); });\n",
    'src/lib/t2-bad.test.js': "it.only('LIB-01 js focus', () => { expect(1).toBe(1); });\n",
    'src/lib/mixed-tier.test.ts': [
      "it('LIB-01 t2 first', () => { expect(1).toBe(1); });",
      "it('CORE-01 t1 second', () => { expect(2).toMatchSnapshot(); });",
    ].join('\n'),
    'src/lib/untagged.test.ts': [
      "it('no spec id', () => { expect(1).toMatchSnapshot(); });",
      "it('NOPE-99 unknown', () => { fc.assert(p, { seed: 9, interruptAfterTimeLimit: 500 }); expect(1).toBe(1); });",
    ].join('\n'),
    'src/core/clock-fake-timers.test.ts': [
      "it('CORE-01 fake timers', () => {",
      '  vi.useFakeTimers();',
      '  const x = [new Date(), Date.now(), performance.now(), process.hrtime(), hrtime.bigint()];',
      '  const r = Math.random();',
      '  expect([x, r]).toHaveLength(2);',
      '});',
    ].join('\n'),
    'src/core/clock-set-time.test.ts': "it('CORE-01 set time', () => { vi.setSystemTime (new Date('2026-01-01')); expect(Date.now()).toBe(1); });\n",
    'src/core/clock-nonboundary.test.ts': "it('CORE-01 fake prefix', () => { myvi.useFakeTimers(); expect(Date.now()).toBe(1); });\n",
    'src/core/clock-nocall.test.ts': "it('CORE-01 no call', () => { const f = vi.useFakeTimers; expect([f, Date.now()]).toHaveLength(2); });\n",
    'src/core/rand-seed.test.ts': [
      "it('CORE-01 seed exempt', () => {",
      '  const SEED = 42;',
      '  const x = [SEED, Math.random(), crypto.randomUUID(), Date.now()];',
      '  expect(x).toHaveLength(4);',
      '});',
    ].join('\n'),
    'src/core/rand-spy-math.test.ts': "it('CORE-01 spy math', () => { vi.spyOn(Math, 'random'); expect(Math.random()).toBe(1); });\n",
    'src/core/rand-spy-crypto.test.ts': "it('CORE-01 spy crypto', () => { vi.spyOn( crypto, 'randomUUID'); expect(crypto.randomUUID()).toBe('a'); });\n",
    'src/core/rand-mock.test.ts': "it('CORE-01 mock', () => { fn.mockReturnValue(1); expect(Math.random()).toBe(1); });\n",
    'src/core/rand-nonboundary.test.ts': [
      "it('CORE-01 nonboundary', () => {",
      "  const noseed = 1; const seeded = 2; vi.spyOn(Mathx, 'random');",
      '  expect([noseed, seeded, Math.random()]).toHaveLength(3);',
      '});',
    ].join('\n'),
    'src/core/prop-ok.test.ts': [
      "it('CORE-01 normal first', () => { expect(1).toBe(1); });",
      "it('PROP-01 seeded prop', () => { fc.assert(p, { seed: 7, interruptAfterTimeLimit: 500 }); expect(1).toBe(1); });",
    ].join('\n'),
    'src/core/prop-unseeded.test.ts': "it('PROP-01 unseeded', () => { fc.assert(p, { numRuns: 50 }); expect(1).toBe(1); });\n",
    'src/core/broken.test.ts': "it('CORE-01 broken', () => {\n",
    'src/core/prod-bad.ts': `${STRYKER_COMMENT}\r\nexport const v = Date.now() + Math.random();\r\n`,
    'src/core/prod-clean.ts': 'export const ok = Date.now() + Math.random();\n',
    'e2e/flow.spec.ts': `${STRYKER_COMMENT}\nit.only('CUJ-01 e2e', () => { expect(Date.now()).toBe(1); });\n`,
    'scripts/outside.ts': `${STRYKER_COMMENT}\nit.only('CORE-01 ignored', () => { expect(Date.now()).toBe(1); });\n`,
    'src/core/not-code.test.md': "it.only('CORE-01 md', () => {});\n",
    'src/core/not-code.test.ts.bak': "it.only('CORE-01 bak', () => {});\n",
  };
}
