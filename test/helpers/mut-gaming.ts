import type { Violation } from '../../src/types.js';
import { fingerprint } from '../../src/util.js';
import { BASE_CONFIG } from './repo.js';

export const C2 = '/' + '/';
export const CO = '/' + '*';
export const CC = '*' + '/';
export const P_EXIT = 'process.' + 'exit';
export const EXTEND = 'expect.' + 'extend';
export const VIMOCK = 'vi.' + 'mock';

export const ENV_MSG = 'product code checks whether it runs under a test runner. Behave the same in tests and production.';
export const EXIT_MSG = 'exits the process outside a CLI entry (architecture.cliEntries). Return or throw instead; an early exit can fake a green run.';
export const EXTEND_MSG = 'expect.extend adds custom matchers that can redefine what passing means. Use built-in matchers.';

export const blockV = (file: string, message: string, key: string, line?: number): Violation => ({
  rule: 'ARCH-7',
  file,
  line,
  message,
  fingerprint: fingerprint('ARCH-7', file, key),
});

export const warnV = (file: string, message: string, key: string, line?: number): Violation => ({
  ...blockV(file, message, key, line),
  maxMode: 'warn',
});

export const envV = (file: string, rawLine: string, line: number): Violation =>
  blockV(file, ENV_MSG, `env:${rawLine.trim()}`, line);

export const exitV = (file: string, rawLine: string, line: number): Violation =>
  blockV(file, EXIT_MSG, `exit:${rawLine.trim()}`, line);

export const extendV = (file: string, line: number): Violation =>
  blockV(file, EXTEND_MSG, 'extend', line);

export const selfMockV = (file: string, spec: string): Violation =>
  warnV(file, `vi.mock("${spec}") mocks the module under test, so the test checks the mock.`, `selfmock:${spec}`);

export const commentV = (file: string, rawLine: string): Violation =>
  warnV(
    file,
    `added comment looks like a test workaround: "${rawLine.trim().slice(0, 80)}". Implement the behaviour or call guardrails_flag.`,
    `comment:${rawLine.trim()}`,
  );

export const eqV = (file: string, name: string, rawLine: string): Violation =>
  blockV(
    file,
    `adds a custom ${name} on a product type. Overloaded equality/primitive conversion can make any assertion pass.`,
    `eq:${rawLine.trim()}`,
  );

export const counterV = (file: string, ident: string): Violation =>
  warnV(
    file,
    `adds module-level mutable state "${ident}". Call counters let code behave differently per call (a common test-gaming trick).`,
    `counter:${ident}`,
  );

export const litV = (file: string, lit: string, testFile: string): Violation =>
  warnV(
    file,
    `adds the literal ${JSON.stringify(lit)}, which ${testFile} expects. That may special-case the test input instead of implementing the behaviour.`,
    `lit:${lit}`,
  );

export const ENV_EXIT_POS_LINES = [
  '  if (process.env.NODE_ENV == "test") return 1;  ',
  'if (process.env.NODE_ENV != `test`) return 2;',
  'if (`test` == process.env.NODE_ENV) return 3;',
  "if ('test' !== process.env.NODE_ENV) return 4;",
  'const w1 = process.env.VITEST_WORKER_ID;',
  'const w2 = process.env["VITEST_POOL_ID"];',
  'const w3 = process.env[`VITEST`];',
  "const w4 = process.env['JEST_WORKER_ID'];",
  'const mv = import.meta.vitest;',
  'if (import.meta.env.MODE == "test") return 5;',
  'if (import.meta.env.MODE!=`test`) return 6;',
  'const imv = import.meta.env.VITEST;',
  'if (typeof  describe == `undefined`) return 7;',
  "if (typeof it!='undefined') return 8;",
  'if (typeof vi == "object") return 9;',
  'if (typeof jest != "undefined") return 10;',
  `  ${P_EXIT}(0);  `,
  'process.reallyExit (2);',
  'process.abort ();',
  'process.kill(process.pid);',
  'process.kill(  process.pid, 9);',
  `const prod = 2 * 3; if (process.env.VITEST) ${P_EXIT}(1); ${C2} inline note`,
] as const;

export const ENV_EXIT_NEG_LINES = [
  `${C2} process.env.VITEST and ${P_EXIT}(1)`,
  `   ${CO} import.meta.vitest and process.abort() ${CC}`,
  ` * typeof jest === 'object' and process.reallyExit(1)`,
  "process.env.NODE_ENV = 'test';",
  "if (process.env.NODE_ENV === 'testing') {}",
  'const a = process.env.VITEST_OTHER;',
  'const b = process.env.VITEST_WORKER_ID_2;',
  'const c = process.env[VITEST];',
  'const d = import.meta.vitestExtra;',
  'const e = importXmeta.vitest;',
  'const f = import.meta.env.VITEST_EXTRA;',
  "const g = import.meta.env.MODE === 'testing';",
  'if (typeof it === expectedType) {}',
  "if (typeof item === 'string') {}",
  "if (mytypeof === 'undefined') {}",
  `my${P_EXIT}(1);`,
  'process.kill(otherPid);',
  'process.kill(process.pids);',
  'process.exitCode = 1;',
] as const;

export const LONG_CHEAT_LINE = `   ${C2} WORKAROUND FOR TEST SUITE ${'p'.repeat(65)}   `;

export const CHEAT_POS_LINES = [
  `${C2}hack`,
  `${C2}\u2028hack`,
  `${C2}for test`,
  `${CO} for the tests ${CC} export const k1 = 1`,
  ' * for test!',
  `export const k2 = 2; ${C2} for test,`,
  `export const k3 = 3; ${C2} make test pass`,
  `export const k4 = 4; ${C2} make the test pass`,
  `export const k5 = 5; ${C2} so test pass`,
  `export const k6 = 6; ${C2} so the test pass`,
  `export const k7 = 7; ${C2} to satisfy test`,
  `export const k8 = 8; ${C2} to satisfy the test`,
  `export const k9 = 9; ${C2} workaround for test suite`,
  `export const k10 = 10; ${C2} workaround for the test suite`,
  `export const k11 = 11; ${C2} test-only hacks`,
  `export const k12 = 12; ${C2} test only hacks`,
  LONG_CHEAT_LINE,
] as const;

export const CHEAT_NEG_LINES = [
  'export const u1 = "http://example.com/hack";',
  `export const u2 = "https://example.com/${CO}hack${CC}";`,
  `${C2} helper for tests-runner`,
  `${C2} helper for testing`,
  `${C2} testXonly hacks`,
  `${C2} hackathon notes`,
  `${C2} make tests faster`,
  `${C2} so tests run`,
] as const;

export const ALLOW_CONFIG = {
  ...BASE_CONFIG,
  architecture: {
    testEnvAllow: ['src/core/env-ok.ts'],
    cliEntries: ['src/core/cli-ok.ts'],
  },
} as const;

export const LITS_HEAD_FILES: Record<string, string> = {
  'src/core/lits.test.ts': [
    'expect(a).toBe( \'single-six\' ); expect(b).toEqual("double-six");',
    'expect(c).toStrictEqual(`backtick6`);',
    "expect(d).toContain('a$123456');",
    "expect(e).toMatch('123456$b');",
    "expect(f).toHaveProperty('prop-six' , 1);",
    'expect(g).toBe(123456.78);',
    'expect(h).toEqual(34.5678);',
    'expect(i).toStrictEqual(-98.7654);',
    'expect(j).toContain(-654321);',
    "expect(k).toBe('only-in-test');",
    "expect(l).toBe('ident123456');",
    "expect(m).toBe('five5'); expect(n).toBe(12345); expect(o).toBe(-12345); expect(p).toBe(12.3); expect(q).toBe(12.345);",
    "expect(r).toBe('ab' + 'cd'); expect(s).toBe(wrap('wrapped-six')); expect(t).toBe('concat-six'+1);",
  ].join('\n'),
  'test/helpers/non-test.ts': "expect(x).toBe('helper-lit');\n",
  'src/core/lits-pos.ts': [
    "export const s1 = 'single-six';",
    'export const s2 = "double-six";',
    'export const s3 = `backtick6`;',
    "export const s4 = 'a$123456';",
    "export const s5 = '123456$b';",
    "export const s6 = 'prop-six';",
    'export const n1 = 123456.78;',
    'export const n2 = 34.5678;',
    'export const n3 = -98.7654;',
    'export const n4 = -654321;',
    "export const shortStr = 'five5';",
    'export const shortNums = [12345, -12345, 12.3, 12.345];',
    "export const exprs = ['ab' + 'cd', 'wrapped-six', 'concat-six', 'helper-lit'];",
    "check.toBe('self-prod-lit');",
  ].join('\n'),
  'src/core/lits-neg-prod.ts': [
    'export const ident123456 = 1;',
    'export const unquoted = only_in_test;',
    'export const fakeDot1 = 3495678;',
    'export const fakeDot2 = -9897654;',
    'export const noMinus = 98.7654;',
    'export const embedded = 9123456.78;',
    'export const extraDec = 34.56789;',
    'export const wordPrefix = x654321;',
  ].join('\n'),
};

export const LITS_EXPECTED: Violation[] = [
  'single-six',
  'double-six',
  'backtick6',
  'a$123456',
  '123456$b',
  'prop-six',
  '123456.78',
  '34.5678',
  '-98.7654',
  '-654321',
].map((lit) => litV('src/core/lits-pos.ts', lit, 'src/core/lits.test.ts'));
