import { defineConfig } from 'vitest/config';

// Under Stryker (GUARDRAILS_MUTATION=1) skip suites that spawn whole tool runs (vitest, ESLint,
// depcruise, jscpd, git worktrees) or the built CLI (test/e2e). Nothing is excluded from mutation
// testing any more: the checks these suites cover are killed by the fast *-mutants.test.ts suites.
const SLOW = ['test/clock.test.ts', 'test/lint.test.ts', 'test/arch.test.ts', 'test/failbase.test.ts', 'test/holdout.test.ts', 'test/dup.test.ts', 'test/e2e/**'];

export default defineConfig({
  test: {
    include: ['test/**/*.test.ts'],
    exclude: process.env.GUARDRAILS_MUTATION === '1' ? SLOW : [],
    setupFiles: ['./src/vitest-setup.ts'],
    testTimeout: 60_000,
    pool: 'forks',
    env: { GUARDRAILS_OFFLINE: '1' },
  },
});
