import { basename } from 'node:path';
import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { diffAgainst, isHelperFile, isProductFile, isTestFile, listFiles, matchesAny, readText, violation, type FileDiff } from '../util.js';

/** ARCH-7 block: product code that detects the test runner. */
const TEST_ENV = [
  /process\.env\.NODE_ENV\s*[!=]==?\s*['"`]test['"`]/,
  /['"`]test['"`]\s*[!=]==?\s*process\.env\.NODE_ENV/,
  /process\.env(?:\.|\[['"`])(VITEST|VITEST_WORKER_ID|VITEST_POOL_ID|JEST_WORKER_ID)\b/,
  /import\.meta\.vitest\b/,
  /import\.meta\.env\.(MODE\s*[!=]==?\s*['"`]test['"`]|VITEST\b)/,
  /\btypeof\s+(jest|vi|describe|it)\s*[!=]==?\s*['"`]/,
];
const EXIT = /\bprocess\.(exit|reallyExit|abort)\s*\(|\bprocess\.kill\(\s*process\.pid\b/;
const EQUALITY_OVERRIDE = /^\s*(?:(?:public|private|protected|static|override)\s+)*(?:valueOf|equals|\[\s*Symbol\.toPrimitive\s*\])\s*(?:\(|:|=)/;
const EXPECT_EXTEND = /\bexpect\.extend\s*\(/;
const COUNTER = /^(?:export\s+)?(?:let|var)\s+(\w+)\s*(?::\s*number\s*)?=\s*0\s*;?\s*$/;
const CHEAT_COMMENT = /(?:^|\s)(?:\/\/|\/\*|\*)\s?.*\b(for (the )?tests?\b|make (the )?tests? pass|so (the )?tests? pass|to satisfy (the )?tests?|workaround for (the )?tests?|hack\b|test[- ]only hack)/i;
const EXPECTED_LITERAL = /\.(?:toBe|toEqual|toStrictEqual|toContain|toMatch|toHaveProperty)\(\s*(?:(['"`])((?:(?!\1).){6,}?)\1|(-?\d{6,}(?:\.\d+)?|-?\d+\.\d{4,}))\s*[,)]/g;

const warn = (x: Violation): Violation => ({ ...x, maxMode: 'warn' });

export function checkGaming(ctx: CheckContext, files = listFiles(ctx.cwd)): CheckOutput {
  const cfg = ctx.config;
  const v: Violation[] = [];
  const product = files.filter((f) => isProductFile(cfg, f));
  const testish = files.filter((f) => isTestFile(cfg, f) || (isHelperFile(cfg, f) && /\.(t|j)sx?$/.test(f)));

  for (const f of product) {
    const lines = readText(ctx.cwd, f).split('\n');
    lines.forEach((line, i) => {
      if (/^\s*(\/\/|\/?\*)/.test(line)) return;
      if (!matchesAny(f, cfg.architecture.testEnvAllow) && TEST_ENV.some((re) => re.test(line)))
        v.push(violation('ARCH-7', f, 'product code checks whether it runs under a test runner. Behave the same in tests and production.', `env:${line.trim()}`, i + 1));
      if (!matchesAny(f, cfg.architecture.cliEntries) && EXIT.test(line))
        v.push(violation('ARCH-7', f, 'exits the process outside a CLI entry (architecture.cliEntries). Return or throw instead; an early exit can fake a green run.', `exit:${line.trim()}`, i + 1));
    });
  }
  for (const f of testish) {
    const lines = readText(ctx.cwd, f).split('\n');
    const i = lines.findIndex((l) => EXPECT_EXTEND.test(l));
    if (i >= 0) v.push(violation('ARCH-7', f, 'expect.extend adds custom matchers that can redefine what passing means. Use built-in matchers.', 'extend', i + 1));
    const mocked = [...readText(ctx.cwd, f).matchAll(/vi\.mock\(\s*['"`]([^'"`]+)['"`]/g)].map((m) => m[1]);
    const subject = basename(f).replace(/\.(test|spec)\.[^.]+$/, '');
    for (const m of mocked)
      if (basename(m).replace(/\.[^.]+$/, '') === subject)
        v.push(warn(violation('ARCH-7', f, `vi.mock("${m}") mocks the module under test, so the test checks the mock.`, `selfmock:${m}`)));
  }

  if (ctx.base) diffSignatures(ctx, diffAgainst(ctx.cwd, ctx.base), v);
  return { violations: v };
}

function diffSignatures(ctx: CheckContext, diff: Record<string, FileDiff>, v: Violation[]): void {
  const cfg = ctx.config;
  const productAdded = new Map<string, string[]>();
  const expected = new Map<string, string>();
  for (const [file, d] of Object.entries(diff)) {
    if (d.status === 'deleted') continue;
    const isProduct = isProductFile(cfg, file);
    for (const line of d.added) {
      if (CHEAT_COMMENT.test(line) && isProduct)
        v.push(warn(violation('ARCH-7', file, `added comment looks like a test workaround: "${line.trim().slice(0, 80)}". Implement the behaviour or call guardrails_flag.`, `comment:${line.trim()}`)));
    }
    if (isProduct) {
      productAdded.set(file, d.added);
      for (const line of d.added) {
        if (EQUALITY_OVERRIDE.test(line))
          v.push(violation('ARCH-7', file, `adds a custom ${line.trim().split(/[\s(:=]/)[0]} on a product type. Overloaded equality/primitive conversion can make any assertion pass.`, `eq:${line.trim()}`));
        const c = COUNTER.exec(line);
        if (c) v.push(warn(violation('ARCH-7', file, `adds module-level mutable state "${c[1]}". Call counters let code behave differently per call (a common test-gaming trick).`, `counter:${c[1]}`)));
      }
    }
    if (isTestFile(cfg, file))
      for (const line of d.added) for (const m of line.matchAll(EXPECTED_LITERAL)) expected.set(m[2] ?? m[3], file);
  }
  for (const [lit, testFile] of expected) {
    for (const [file, added] of productAdded) {
      const quoted = /^-?[\d.]+$/.test(lit) ? new RegExp(`(^|[^\\w.])${lit.replace(/[.-]/g, '\\$&')}($|[^\\w.])`) : null;
      const hit = added.some((l) => (quoted ? quoted.test(l) : l.includes(`'${lit}'`) || l.includes(`"${lit}"`) || l.includes(`\`${lit}\``)));
      if (hit) v.push(warn(violation('ARCH-7', file, `adds the literal ${JSON.stringify(lit)}, which ${testFile} expects. That may special-case the test input instead of implementing the behaviour.`, `lit:${lit}`)));
    }
  }
}
