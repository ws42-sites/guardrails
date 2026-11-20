import { describe, expect, it } from 'vitest';
import { checkLint } from '../src/checks/lint.js';
import { fingerprint } from '../src/util.js';
import {
  expectedViolation,
  LINT_MUT_CONFIG,
  megaFixtureFiles,
  normalizeViolations,
} from './helpers/mut-lint.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

describe('LINT-02 checkLint enforces TEST-5 patterns, modes, exemptions and config isolation', () => {
  it('LINT-02 blocks and warns on all TEST-5 patterns with exact file, line, maxMode, message and fingerprint', async () => {
    const files = megaFixtureFiles();
    const out = await checkLint(repo(files, LINT_MUT_CONFIG).ctx());
    const actual = normalizeViolations(out.violations);

    const b = 'src/core/t1-blocks.test.ts';
    const bLines = files[b].split('\n');
    const w = 'src/core/t1-warns.test.tsx';
    const wLines = files[w].split('\n');
    const jsBad = 'src/lib/t2-bad.test.js';
    const jsxBad = 'src/lib/t2-jsx.test.jsx';
    const mixed = 'src/lib/mixed-tier.test.ts';
    const untagged = 'src/lib/untagged.test.ts';
    const clockFake = 'src/core/clock-fake-timers.test.ts';
    const clockPrefix = 'src/core/clock-nonboundary.test.ts';
    const clockNocall = 'src/core/clock-nocall.test.ts';
    const randSeed = 'src/core/rand-seed.test.ts';
    const randNonbound = 'src/core/rand-nonboundary.test.ts';
    const propUnseeded = 'src/core/prop-unseeded.test.ts';
    const broken = 'src/core/broken.test.ts';
    const prodBad = 'src/core/prod-bad.ts';

    const byFile = (f: string) => actual.filter((v) => v.file === f);
    expect([...new Set(actual.map((v) => v.file))].sort()).toEqual([
      broken,
      clockFake,
      clockNocall,
      clockPrefix,
      prodBad,
      propUnseeded,
      randNonbound,
      randSeed,
      b,
      w,
      mixed,
      jsBad,
      jsxBad,
      untagged,
    ]);

    expect(byFile(b)).toEqual([
      expectedViolation(b, 6, 'vitest/no-focused-tests', 'Focused tests are not allowed.', bLines[5].trim()),
      expectedViolation(b, 7, 'vitest/no-disabled-tests', 'Disabled test', bLines[6].trim()),
      expectedViolation(b, 8, 'vitest/no-disabled-tests', 'Disabled test', bLines[7].trim()),
      expectedViolation(b, 9, 'no-restricted-syntax', 'test.todo is banned; write the test or track it in an issue.', bLines[8].trim()),
      expectedViolation(b, 10, 'no-restricted-syntax', 'each([]) with an empty table runs nothing.', bLines[9].trim()),
      expectedViolation(b, 11, 'no-restricted-syntax', 'Real clock in a test: use vi.useFakeTimers() and vi.setSystemTime(new Date("2026-01-01")).', bLines[10].trim()),
      expectedViolation(b, 11, 'no-restricted-syntax', 'Real clock in a test: use vi.useFakeTimers() and vi.setSystemTime().', bLines[10].trim()),
      expectedViolation(b, 11, 'no-restricted-syntax', 'Real clock in a test: use fake timers.', bLines[10].trim()),
      expectedViolation(b, 11, 'no-restricted-syntax', 'Real clock in a test: use fake timers.', bLines[10].trim()),
      expectedViolation(b, 11, 'no-restricted-syntax', 'Real clock in a test: use fake timers.', bLines[10].trim()),
      expectedViolation(b, 12, 'no-restricted-syntax', 'Randomness in a test: use fixed inputs (or a seeded generator).', bLines[11].trim()),
      expectedViolation(b, 12, 'no-restricted-syntax', 'Randomness in a test: use fixed IDs/bytes.', bLines[11].trim()),
      expectedViolation(b, 12, 'no-restricted-syntax', 'Randomness in a test: use fixed IDs/bytes.', bLines[11].trim()),
      expectedViolation(b, 13, 'guardrails/no-generated-inputs', 'fast-check run without a fixed seed and numRuns <= 20. Unseeded property runs are random; seed them (and mark the spec properties: true) or use fixed cases.', bLines[12].trim()),
      expectedViolation(b, 14, 'guardrails/no-stryker-disable', '"Stryker disable" comments are banned; they hide surviving mutants. Kill the mutant with a real assertion.', bLines[13].trim()),
      expectedViolation(b, 15, 'no-restricted-syntax', 'Snapshots are banned in T1 tests; assert the exact values that matter.', bLines[14].trim(), 'warn'),
      expectedViolation(b, 15, 'no-restricted-syntax', 'Snapshots are banned in T1 tests; assert the exact values that matter.', bLines[14].trim(), 'warn'),
    ]);

    expect(byFile(w)).toEqual([
      expectedViolation(w, 3, 'no-restricted-syntax', 'Class-name/style assertions are banned; visuals are checked by screenshot diff.', wLines[2].trim(), 'warn'),
      expectedViolation(w, 4, 'no-restricted-syntax', 'Class-name/style assertions are banned; visuals are checked by screenshot diff.', wLines[3].trim(), 'warn'),
      expectedViolation(w, 5, 'no-restricted-syntax', 'Hand-rolled fake servers are banned; test through the storage interface or a fixture implementation.', wLines[4].trim(), 'warn'),
      expectedViolation(w, 6, 'no-restricted-syntax', 'Spying on Date/Response is banned; use vi.useFakeTimers()/setSystemTime or a real Response.', wLines[5].trim(), 'warn'),
      expectedViolation(w, 7, 'no-restricted-syntax', 'Spying on Date/Response is banned; use vi.useFakeTimers()/setSystemTime or a real Response.', wLines[6].trim(), 'warn'),
      expectedViolation(w, 8, 'no-restricted-syntax', 'Spying on crypto.subtle or a prototype is banned; test the real behaviour.', wLines[7].trim(), 'warn'),
      expectedViolation(w, 9, 'no-restricted-syntax', 'Spying on crypto.subtle or a prototype is banned; test the real behaviour.', wLines[8].trim(), 'warn'),
      expectedViolation(w, 10, 'guardrails/no-generated-inputs', 'Loop over 21 iterations in a test. Generated input loops are padding; use a small it.each table of meaningful cases.', wLines[9].trim(), 'warn'),
      expectedViolation(w, 11, 'guardrails/no-generated-inputs', 'Loop over 25 iterations in a test. Generated input loops are padding; use a small it.each table of meaningful cases.', wLines[10].trim(), 'warn'),
      expectedViolation(w, 12, 'guardrails/no-generated-inputs', 'Seeded fast-check property run. Allowed only for specs marked properties: true, with interruptAfterTimeLimit instead of a run cap.', wLines[11].trim(), 'warn'),
    ]);

    expect(byFile(mixed)).toEqual([
      expectedViolation(mixed, 2, 'no-restricted-syntax', 'Snapshots are banned in T1 tests; assert the exact values that matter.', files[mixed].split('\n')[1].trim(), 'warn'),
    ]);
    expect(byFile(jsBad)).toEqual([
      expectedViolation(jsBad, 1, 'vitest/no-focused-tests', 'Focused tests are not allowed.', files[jsBad].split('\n')[0].trim()),
    ]);
    expect(byFile(jsxBad)).toEqual([
      expectedViolation(jsxBad, 1, 'no-restricted-syntax', 'Class-name/style assertions are banned; visuals are checked by screenshot diff.', files[jsxBad].split('\n')[0].trim(), 'warn'),
    ]);
    expect(byFile(untagged)).toEqual([
      expectedViolation(untagged, 2, 'guardrails/no-generated-inputs', 'Seeded fast-check property run. Allowed only for specs marked properties: true, with interruptAfterTimeLimit instead of a run cap.', files[untagged].split('\n')[1].trim(), 'warn'),
    ]);
    expect(byFile(clockFake)).toEqual([
      expectedViolation(clockFake, 4, 'no-restricted-syntax', 'Randomness in a test: use fixed inputs (or a seeded generator).', files[clockFake].split('\n')[3].trim()),
    ]);
    expect(byFile(clockPrefix)).toEqual([
      expectedViolation(clockPrefix, 1, 'no-restricted-syntax', 'Real clock in a test: use vi.useFakeTimers() and vi.setSystemTime().', files[clockPrefix].split('\n')[0].trim()),
    ]);
    expect(byFile(clockNocall)).toEqual([
      expectedViolation(clockNocall, 1, 'no-restricted-syntax', 'Real clock in a test: use vi.useFakeTimers() and vi.setSystemTime().', files[clockNocall].split('\n')[0].trim()),
    ]);
    expect(byFile(randSeed)).toEqual([
      expectedViolation(randSeed, 3, 'no-restricted-syntax', 'Real clock in a test: use vi.useFakeTimers() and vi.setSystemTime().', files[randSeed].split('\n')[2].trim()),
    ]);
    expect(byFile(randNonbound)).toEqual([
      expectedViolation(randNonbound, 3, 'no-restricted-syntax', 'Randomness in a test: use fixed inputs (or a seeded generator).', files[randNonbound].split('\n')[2].trim()),
    ]);
    expect(byFile(propUnseeded)).toEqual([
      expectedViolation(propUnseeded, 1, 'guardrails/no-generated-inputs', 'fast-check run without a fixed seed and numRuns <= 20. Unseeded property runs are random; seed them (and mark the spec properties: true) or use fixed cases.', files[propUnseeded].split('\n')[0].trim()),
    ]);
    expect(byFile(broken)).toEqual([
      {
        rule: 'TEST-5',
        file: broken,
        line: undefined,
        maxMode: undefined,
        message: "could not lint: Parsing error: '}' expected.",
        fingerprint: fingerprint('TEST-5', broken, 'fatal'),
      },
    ]);
    expect(byFile(prodBad)).toEqual([
      expectedViolation(prodBad, 1, 'guardrails/no-stryker-disable', '"Stryker disable" comments are banned; they hide surviving mutants. Kill the mutant with a real assertion.', files[prodBad].split('\n')[0].trim()),
    ]);
  });

  it.each([
    {
      name: 'empty repo returns no violations',
      files: { 'README.md': '# empty\n' } as Record<string, string>,
      explicit: undefined as string[] | undefined,
      expected: [] as ReturnType<typeof normalizeViolations>,
    },
    {
      name: 'product-only repo still checks Stryker-disable without applying test rules',
      files: { 'src/core/only-prod.ts': '// ' + 'Stryker disable next-line\nexport const x = Date.now() + Math.random();\n' },
      explicit: undefined,
      expected: [
        expectedViolation(
          'src/core/only-prod.ts',
          1,
          'guardrails/no-stryker-disable',
          '"Stryker disable" comments are banned; they hide surviving mutants. Kill the mutant with a real assertion.',
          '// ' + 'Stryker disable next-line',
        ),
      ],
    },
    {
      name: 'T2-only tests repo runs test rules without T1 snapshot rules or product files',
      files: {
        'src/lib/only-t2.test.ts': `it('LIB-01 snap', () => { expect(1).toMatchSnapshot(); expect(Math.random()).toBe(1); });\n`,
      },
      explicit: undefined,
      expected: [
        expectedViolation(
          'src/lib/only-t2.test.ts',
          1,
          'no-restricted-syntax',
          'Randomness in a test: use fixed inputs (or a seeded generator).',
          `it('LIB-01 snap', () => { expect(1).toMatchSnapshot(); expect(Math.random()).toBe(1); });`,
        ),
      ],
    },
    {
      name: 'explicit files argument restricts linting to passed files',
      files: {
        'src/core/a.ts': '// ' + 'Stryker disable all\nexport const a = 1;\n',
        'src/core/b.ts': '// ' + 'Stryker disable all\nexport const b = 2;\n',
      },
      explicit: ['src/core/a.ts'],
      expected: [
        expectedViolation(
          'src/core/a.ts',
          1,
          'guardrails/no-stryker-disable',
          '"Stryker disable" comments are banned; they hide surviving mutants. Kill the mutant with a real assertion.',
          '// ' + 'Stryker disable all',
        ),
      ],
    },
  ])('LINT-02 $name', async ({ files, explicit, expected }) => {
    const r = repo(files, BASE_CONFIG);
    const out = explicit ? await checkLint(r.ctx(), explicit) : await checkLint(r.ctx());
    expect(normalizeViolations(out.violations)).toEqual(expected);
  });
});
