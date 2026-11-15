import { createRequire } from 'node:module';
import { ESLint, type Linter } from 'eslint';
import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { isProductFile, isTestFile, listFiles, readText, violation } from '../util.js';
import { plugin, SNAPSHOT_SYNTAX, TEST_SYNTAX } from '../eslint/plugin.js';
import { indexTests } from './tests.js';

const require = createRequire(import.meta.url);

const restricted = (items: { selector: string; message: string }[]): Linter.RuleEntry => ['error', ...items.map(({ selector, message }) => ({ selector, message }))];

const CLOCK_EXEMPT = /\bvi\.(useFakeTimers|setSystemTime)\s*\(/;
const RANDOM_EXEMPT = /\bseed\b|vi\.spyOn\(\s*(Math|crypto)\b|mockReturnValue/i;

function baseConfig(): Linter.Config {
  return {
    root: true,
    parser: require.resolve('@typescript-eslint/parser'),
    parserOptions: { ecmaVersion: 'latest', sourceType: 'module', ecmaFeatures: { jsx: true } },
    plugins: ['vitest', 'guardrails'],
    rules: {},
  };
}

/** TEST-5: ESLint-backed bans on test files; Stryker-disable ban on product files. */
export async function checkLint(ctx: CheckContext, files = listFiles(ctx.cwd)): Promise<CheckOutput> {
  const cfg = ctx.config;
  const tests = files.filter((f) => isTestFile(cfg, f) && /\.(t|j)sx?$/.test(f));
  const product = files.filter((f) => isProductFile(cfg, f));
  if (!tests.length && !product.length) return { violations: [] };

  const index = indexTests(ctx, tests);
  const t1Tests = tests.filter((f) => (index.byFile[f] ?? []).some((t) => t.specId && cfg.specs.items[t.specId]?.tier === 'T1'));

  const eslint = new ESLint({
    cwd: ctx.cwd,
    useEslintrc: false,
    ignore: false,
    allowInlineConfig: false,
    plugins: { vitest: require('eslint-plugin-vitest'), guardrails: plugin } as unknown as Record<string, ESLint.Plugin>,
    overrideConfig: {
      ...baseConfig(),
      rules: { 'guardrails/no-stryker-disable': 'error' },
      overrides: [
        {
          files: tests.length ? tests : ['__none__'],
          rules: {
            'vitest/no-focused-tests': 'error',
            'vitest/no-disabled-tests': 'error',
            'guardrails/no-generated-inputs': 'error',
            'no-restricted-syntax': restricted(TEST_SYNTAX),
          },
        },
        {
          files: t1Tests.length ? t1Tests : ['__none__'],
          rules: { 'no-restricted-syntax': restricted([...TEST_SYNTAX, SNAPSHOT_SYNTAX]) },
        },
      ],
    },
  });

  const results = await eslint.lintFiles([...tests, ...product]);
  const v: Violation[] = [];
  for (const r of results) {
    const file = r.filePath.slice(ctx.cwd.length + 1).split('\\').join('/');
    const src = readText(ctx.cwd, file);
    const lines = src.split('\n');
    for (const m of r.messages) {
      if (m.fatal) {
        v.push(violation('TEST-5', file, `could not lint: ${m.message}`, 'fatal'));
        continue;
      }
      const tag = TEST_SYNTAX.find((s) => m.message.startsWith(s.message))?.tag;
      if (tag === 'clock' && CLOCK_EXEMPT.test(src)) continue;
      if (tag === 'random' && RANDOM_EXEMPT.test(src)) continue;
      const msg = m.message;
      const text = (lines[m.line - 1] ?? '').trim();
      v.push(violation('TEST-5', file, `${msg} (${m.ruleId})`, `${m.ruleId}:${msg}:${text}`, m.line));
    }
  }
  return { violations: v };
}
