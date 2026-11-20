import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { checkGaming } from '../src/checks/gaming.js';
import type { Violation } from '../src/types.js';
import {
  ALLOW_CONFIG,
  C2,
  CHEAT_NEG_LINES,
  CHEAT_POS_LINES,
  commentV,
  counterV,
  ENV_EXIT_NEG_LINES,
  ENV_EXIT_POS_LINES,
  envV,
  eqV,
  exitV,
  EXTEND,
  extendV,
  LITS_EXPECTED,
  LITS_HEAD_FILES,
  litV,
  P_EXIT,
  selfMockV,
  VIMOCK,
} from './helpers/mut-gaming.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

describe('GAME-03 checkGaming ARCH-7 boundary, diff and adversarial verification', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z'));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it.each<{
    label: string;
    config: object;
    files: Record<string, string>;
    explicit?: string[];
    expected: Violation[];
  }>([
    {
      label: 'flags every TEST_ENV and EXIT boundary variant with exact line and trimmed fingerprint',
      config: BASE_CONFIG,
      files: { 'src/core/env-exit.ts': `${ENV_EXIT_POS_LINES.join('\n')}\n` },
      expected: [
        ...ENV_EXIT_POS_LINES.slice(0, 16).map((l, idx) => envV('src/core/env-exit.ts', l, idx + 1)),
        ...ENV_EXIT_POS_LINES.slice(16, 21).map((l, idx) => exitV('src/core/env-exit.ts', l, idx + 17)),
        envV('src/core/env-exit.ts', ENV_EXIT_POS_LINES[21], 22),
        exitV('src/core/env-exit.ts', ENV_EXIT_POS_LINES[21], 22),
      ],
    },
    {
      label: 'ignores comment lines, benign look-alikes and non-product files',
      config: BASE_CONFIG,
      files: {
        'src/core/benign.ts': `${ENV_EXIT_NEG_LINES.join('\n')}\n`,
        'src/core/ok.test.ts': `if (process.env.VITEST) ${P_EXIT}(1);\n`,
        'test/helpers/ok.ts': `if (process.env.VITEST) ${P_EXIT}(1);\n`,
        'src/fixtures/ok.ts': `if (process.env.VITEST) ${P_EXIT}(1);\n`,
        'e2e/ok.spec.ts': `if (process.env.VITEST) ${P_EXIT}(1);\n`,
        'src/core/notes.md': `if (process.env.VITEST) ${P_EXIT}(1);\n`,
        'outside/leak.ts': `if (process.env.VITEST) ${P_EXIT}(1);\n`,
      },
      expected: [],
    },
    {
      label: 'honours testEnvAllow, cliEntries and explicit files subset',
      config: ALLOW_CONFIG,
      files: {
        'src/core/env-ok.ts': `if (process.env.VITEST) ${P_EXIT}(1);\n`,
        'src/core/cli-ok.ts': `if (process.env.VITEST) ${P_EXIT}(1);\n`,
        'src/core/unselected.ts': `if (process.env.VITEST) ${P_EXIT}(1);\n`,
      },
      explicit: ['src/core/env-ok.ts', 'src/core/cli-ok.ts'],
      expected: [
        exitV('src/core/env-ok.ts', `if (process.env.VITEST) ${P_EXIT}(1);`, 1),
        envV('src/core/cli-ok.ts', `if (process.env.VITEST) ${P_EXIT}(1);`, 1),
      ],
    },
  ])('GAME-03 $label', ({ config, files, explicit, expected }) => {
    const r = repo(files, config);
    const out = explicit ? checkGaming(r.ctx(), explicit) : checkGaming(r.ctx());
    expect(out).toEqual({ violations: expected });
  });

  it.each<{ label: string; files: Record<string, string>; expected: Violation[] }>([
    {
      label: 'flags expect.extend on line 1 and line 2 across test and helper extensions and warns on self-mocks',
      files: {
        'src/core/ext-first.test.ts': `${EXTEND} ({ ok: () => ({ pass: true, message: () => '' }) });\n`,
        'test/helpers/ext-ts.ts': `export const h = 1;\n  ${EXTEND}({});\n`,
        'test/helpers/ext-js.js': `${EXTEND}({});\n`,
        'test/helpers/ext-tsx.tsx': `${EXTEND}({});\n`,
        'test/helpers/ext-jsx.jsx': `${EXTEND}({});\n`,
        'src/core/foo.test.bar.test.ts': `${VIMOCK}('./other.helper.js');\n${VIMOCK}('./foo.test.bar.js');\n`,
        'src/core/widget.spec.ts': `${VIMOCK}(\`./widget\`);\n${VIMOCK}("./different.mjs");\n`,
        'src/core/alpha.test.tsx': `${VIMOCK}("../core/alpha.cjs");\n`,
      },
      expected: [
        selfMockV('src/core/alpha.test.tsx', '../core/alpha.cjs'),
        extendV('src/core/ext-first.test.ts', 1),
        selfMockV('src/core/foo.test.bar.test.ts', './foo.test.bar.js'),
        selfMockV('src/core/widget.spec.ts', './widget'),
        extendV('test/helpers/ext-js.js', 1),
        extendV('test/helpers/ext-jsx.jsx', 1),
        extendV('test/helpers/ext-ts.ts', 2),
        extendV('test/helpers/ext-tsx.tsx', 1),
      ],
    },
    {
      label: 'ignores expect.extend look-alikes and non-testish or non-code helper files',
      files: {
        'src/core/lookalike.test.ts': `${EXTEND}Matchers();\nmy${EXTEND}({});\n`,
        'test/helpers/note.ts.txt': `${EXTEND}({});\n`,
        'test/helpers/data.json': `{"call":"${EXTEND}({})"}\n`,
        'src/core/prod.ts': `${EXTEND}({});\n`,
        'e2e/cuj.test.ts': `${EXTEND}({});\n`,
        'README.md': `${EXTEND}({});\n`,
      },
      expected: [],
    },
  ])('GAME-03 $label', ({ files, expected }) => {
    const out = checkGaming(repo(files).ctx());
    const sortKey = (v: Violation) => `${v.file}:${v.line ?? 0}:${v.fingerprint}`;
    expect([...out.violations].sort((a, b) => sortKey(a).localeCompare(sortKey(b)))).toEqual(expected);
  });

  it.each<{
    label: string;
    baseFiles: Record<string, string>;
    removeFiles?: string[];
    headFiles: Record<string, string>;
    expected: Violation[];
  }>([
    {
      label: 'flags equality overrides with spaced modifiers and Symbol.toPrimitive plus let/var counters only when base is set',
      baseFiles: { 'src/core/eq-cnt.ts': 'export class Box {}\n' },
      headFiles: {
        'src/core/eq-cnt.ts': [
          'export class Box {',
          '  public  valueOf() { return 1; }',
          '  private  override  equals: (x: unknown) => boolean = () => true;',
          '  protected  static  valueOf = () => 2;',
          '  [ Symbol.toPrimitive ] = () => 3;  ',
          '}',
          'export  let  spacedHits :  number  =  0 ;  ',
          'let tightHits:number=0',
          'var  spacedVar = 0;',
        ].join('\n'),
      },
      expected: [
        eqV('src/core/eq-cnt.ts', 'valueOf', '  public  valueOf() { return 1; }'),
        eqV('src/core/eq-cnt.ts', 'equals', '  private  override  equals: (x: unknown) => boolean = () => true;'),
        eqV('src/core/eq-cnt.ts', 'valueOf', '  protected  static  valueOf = () => 2;'),
        eqV('src/core/eq-cnt.ts', '[Symbol.toPrimitive]', '  [ Symbol.toPrimitive ] = () => 3;  '),
        counterV('src/core/eq-cnt.ts', 'spacedHits'),
        counterV('src/core/eq-cnt.ts', 'tightHits'),
        counterV('src/core/eq-cnt.ts', 'spacedVar'),
      ],
    },
    {
      label: 'ignores non-matching counters, inline valueOf, test-file counters and deleted files',
      baseFiles: {
        'src/core/gone.ts': 'let oldHits = 0;\n',
        'src/core/gone.test.ts': "it('CORE-01 gone', () => { expect(1).toBe(123456); });\n",
        'src/core/neg.ts': 'export const base = 1;\n',
      },
      removeFiles: ['src/core/gone.ts', 'src/core/gone.test.ts'],
      headFiles: {
        'src/core/neg.ts': [
          'export const base = 1;',
          '  let indented = 0;',
          'const constZero = 0;',
          'let plusOne = 0 + 1;',
          'let hexish = 0x1;',
          'let trailingCode = 0;extra',
          'let stringType: string = 0 as never;',
          'exportlet stuck = 0;',
          'const inline = { valueOf() { return 1; } };',
          'class C { publicvalueOf() {} }',
          'valueOf;',
        ].join('\n'),
        'src/core/neg.test.ts': 'let testCounter = 0;\nclass T {\n  valueOf() { return 1; }\n}\n',
      },
      expected: [],
    },
  ])('GAME-03 $label', ({ baseFiles, removeFiles, headFiles, expected }) => {
    const r = repo(baseFiles).commit();
    if (removeFiles) r.remove(...removeFiles);
    r.write(headFiles);
    expect(checkGaming(r.ctx(null))).toEqual({ violations: [] });
    expect(checkGaming(r.ctx('main'))).toEqual({ violations: expected });
  });

  it.each<{ label: string; headFiles: Record<string, string>; expected: Violation[] }>([
    {
      label: 'flags every cheat comment phrase, delimiter, u2028 separator and 80-char truncation',
      headFiles: { 'src/core/cheat.ts': `export const seed = 1;\n${CHEAT_POS_LINES.join('\n')}\n` },
      expected: CHEAT_POS_LINES.map((l) => commentV('src/core/cheat.ts', l)),
    },
    {
      label: 'ignores URLs, benign test comments and cheat comments in test files',
      headFiles: {
        'src/core/cheat-neg.ts': `export const seed = 1;\n${CHEAT_NEG_LINES.join('\n')}\n`,
        'src/core/cheat-neg.test.ts': `${C2} hack to make the test pass\n`,
      },
      expected: [],
    },
  ])('GAME-03 $label', ({ headFiles, expected }) => {
    const r = repo({ 'src/core/cheat.ts': 'export const seed = 1;\n', 'src/core/cheat-neg.ts': 'export const seed = 1;\n' }).commit();
    r.write(headFiles);
    expect(checkGaming(r.ctx('main'))).toEqual({ violations: expected });
  });

  it.each<{ label: string; headFiles: Record<string, string>; expected: Violation[] }>([
    {
      label: 'matches 6+ char strings, 6+ digit numbers and 4+ decimal floats across matchers and quotes while rejecting look-alikes',
      headFiles: LITS_HEAD_FILES,
      expected: LITS_EXPECTED,
    },
    {
      label: 'matches numeric literal at line start and end while rejecting adjacent decimal dots',
      headFiles: {
        'src/core/edge.test.ts': 'expect(x).toBe(765432);\n',
        'src/core/edge-hit.ts': '765432',
        'src/core/edge-dot.ts': 'export const a = 0.765432;\nexport const b = 765432.1;\n',
      },
      expected: [litV('src/core/edge-hit.ts', '765432', 'src/core/edge.test.ts')],
    },
  ])('GAME-03 $label', ({ headFiles, expected }) => {
    const r = repo({ 'src/core/seed.ts': 'export const seed = 0;\n' }).commit();
    r.write(headFiles);
    expect(checkGaming(r.ctx('main'))).toEqual({ violations: expected });
  });

  it.each<{ label: string; crlf: boolean }>([
    { label: 'CRLF hostile payloads and seeded fuzz inputs', crlf: true },
    { label: 'LF hostile payloads and seeded fuzz inputs', crlf: false },
  ])('GAME-03 handles $label deterministically without canary leakage', ({ crlf }) => {
    const canary = 'CANARY-7b9e4c2a';
    const eol = crlf ? '\r\n' : '\n';
    let state = 0x5eed1234;
    const nextInt = () => {
      state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
      return state;
    };
    const hostileTokens = [
      `SELECT * FROM t WHERE k='${canary}'`,
      `<script>${canary}</script>`,
      `{"__proto__":{"k":"${canary}"}}`,
      `https://u:${canary}@evil.example/hack`,
      `\\u0000${canary}${'z'.repeat(4096)}`,
    ];
    const benignLines = Array.from({ length: 12 }, (_, idx) => `export const h${idx} = ${JSON.stringify(hostileTokens[nextInt() % hostileTokens.length])};`);
    const r = repo({ 'src/core/adv.ts': `export const init = 1;${eol}` }).commit();
    const hitLine = '  if (process.env.VITEST) return 0;  ';
    r.write({ 'src/core/adv.ts': `${[...benignLines, hitLine].join(eol)}${eol}` });
    const first = checkGaming(r.ctx('main'));
    const second = checkGaming(r.ctx('main'));
    expect(second).toEqual(first);
    expect(JSON.stringify(first)).not.toContain(canary);
    expect(first).toEqual({ violations: [envV('src/core/adv.ts', hitLine, 13)] });
  });
});
