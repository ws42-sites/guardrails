import { defineConfig } from 'vitest/config';

// Under Stryker (GUARDRAILS_MUTATION=1) skip suites that spawn whole tool runs (vitest, ESLint,
// depcruise, git worktrees). They are slow per mutant and cover engines, not the mutated rule logic.
const SLOW = ['test/clock.test.ts', 'test/lint.test.ts', 'test/arch.test.ts', 'test/failbase.test.ts', 'test/holdout.test.ts', 'test/dup.test.ts'];

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
