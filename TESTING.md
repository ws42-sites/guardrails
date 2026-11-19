# Testing policy

Shared by every project that uses `@ws42/guardrails`. Projects don't copy this file; `AGENTS.md` links to it. Rule IDs in brackets are enforced (see RULES.md).

## 1. Tests come from specs, not from the code
- Behaviours live in `guardrails.config.json` → `specs.items`: an ID, a tier and one line saying what must hold. Example: `"REVIEW-02": { "tier": "T1", "behaviour": "Expired token returns 410 and writes nothing" }`.
- Every test title (or the `describe` around it) starts with a spec ID: `it("REVIEW-02 expired token returns 410", …)`. IDs match `^[A-Z][A-Z0-9]{1,11}-[0-9]{2,4}$` and must exist in the spec block [TEST-1].
- New behaviour means a new spec entry first. The config is CODEOWNER-protected, so a human approves what gets tested.
- More than 8 tests on one ID is padding. Use `it.each` [TEST-4].

## 2. Journeys come first [JRN-1..6]
Above the specs sit Critical User Journeys: 5-10 complete tasks users actually do, written in plain language ("Client opens the review link and submits a choice"). Each has a priority, steps, the spec IDs it depends on and its routes. Only journeys a human approved gate merges.
- Every approved P0 journey has exactly one e2e test (Playwright, `e2e/`) whose title starts with its ID and walks the steps [JRN-1]. P1 gaps warn; P2 is optional.
- Every spec should serve an approved journey. Orphans warn: they signal padding or a feature nobody asked for [JRN-2].
- Agents never add, edit or approve journeys [JRN-3]. New routes, forms and endpoints should map to a journey [JRN-4].

Executable journeys use `journeys scaffold`, `journeys generate-tests`, and `journeys run --base-url <url>`. Check generated files with `journeys generate-tests --check`; JRN-6 compares the specs and shared runtime byte-for-byte. Declare mocks for writes: the runtime aborts unmocked non-GET/HEAD requests. Approved/client-provided failures gate execution; unapproved inferred/template failures warn.

## 3. Risk tiers decide how much testing
| Tier | What | Expectation |
|---|---|---|
| T1 | Public endpoints, auth, tokens, storage writes, money | Every T1 spec has a test [TEST-2]. Cover failure paths. Mutation score ≥ 70% on changed files [TEST-8]. No snapshots [TEST-5]. Budget ~1.0 test line per product line. |
| T2 | Internal logic | Happy path plus the edges that matter, as `it.each` tables. Untested T2 specs warn. Budget ~0.6. |
| T3 | UI layout | One flow test per page. Visuals are checked against the mocks by screenshot diff, never by class names or snapshots. Budget ~0.4. |

The budget [TEST-3] only warns. It points reviewers at padding; it never fails a PR on its own.

## 4. Banned patterns [TEST-5]
Block (they make tests lie):
- `.only`, `.skip`, `xit`/`xdescribe`, `test.todo`, `each([])`.
- The real clock (`new Date()`, `Date.now()`, `performance.now()`, `process.hrtime`) unless the file uses `vi.useFakeTimers()`/`vi.setSystemTime()`.
- Randomness (`Math.random`, `crypto.randomUUID`, `getRandomValues`) unless seeded or mocked.
- fast-check without a fixed `seed` (and over `numRuns ≤ 20`).
- `// Stryker disable` comments anywhere, and `expect.extend` custom matchers [ARCH-7].

Warn (weak, preference-style; moved from block in v0.2):
- Generated-input loops over 20 iterations.
- Hand-rolled fake servers (`createServer`). Test through the storage interface or its fixture implementation.
- Spying on `Date`, `Response`, `crypto.subtle` or prototypes.
- `toHaveClass`/`toHaveStyle`. Snapshots in T1 tests.

Property tests: a spec with `"properties": true` may use fast-check with a fixed `seed` and `interruptAfterTimeLimit` (≤ `tests.propertyTimeLimitMs`, default 5000) instead of the 20-run cap. Elsewhere seeded property runs warn.

Heuristics that only warn [TEST-5W]: tests with no `expect`, tests that only assert a mock was called, and tests whose only assertions are `toBeDefined`/`toBeTruthy`/`not.toThrow`/`expect(true)`. Every project must add `@ws42/guardrails/vitest-setup` (or its own `expect.hasAssertions()` setup) to `setupFiles` [TEST-9].

## 5. Time [TEST-6]
CI reruns the suite with the clock moved forward 2 years. A test that passes today and fails then is a time bomb, and it blocks. Freeze time in the test.

## 6. Never weaken a test [TEST-7]
In a PR, these block:
- deleting a test that exists on the base branch (needs the `tests-removed-approved` label from a code owner)
- removing an `expect`, or loosening a matcher (`toEqual` → `toMatchObject`/`toContain`, `toBe` → `toBeTruthy`)
- lowering any threshold, mode, tier or allowlist in `guardrails.config.json`, or a coverage/mutation threshold in Vitest or Stryker config

Tests added in the same PR can be changed or deleted freely. Moving a test to another file is not a deletion. Any drop in the total test count versus base warns with the count [TEST-10].

## 7. Tests must fail without the change [TEST-11]
In a PR, every new or changed test runs twice: on the base branch's product code (a git worktree with only the PR's test files copied in) and on head. It must fail on base and pass on head; if you revert the implementation, the test should fail. A test that passes on base blocks unless its spec has `"regressionGuard": true` or a human adds the `allow-characterization` label. Only the changed test files run, so it's fast.

## 8. Conflicts: stop and flag [FLAG-1]
If a test contradicts the spec or can't pass honestly, the agent calls `guardrails_flag` / `guardrails flag` and stops. Flags live in `.guardrails/flags.json` and block merge until a human empties it or adds `flags-resolved`. Agent hooks lock spec-tagged tests and guardrails files, so editing the test is not an option. In ImpossibleBench, letting agents flag cut cheating from 54% to 9%.

## 9. Hidden holdout tests [HOLD-1]
Optional: tests the building agent never sees, in a separate private repo. The PR gate checks them out and runs them; only titles and pass/fail are reported. Their specs are exempt from TEST-2.

## 10. Size
Test files over 300 lines warn [TEST-4]. Split them by behaviour.

## 11. Mutation testing [TEST-8]
Stryker runs on changed files in `mutation.tiers` (T1 by default), incrementally, with pinned mutators. A surviving mutant means no test notices that bug. Add an assertion that would. `guardrails_mutants` lists survivors for the agent; the gate always recomputes the score itself (agents have misreported their own scores). Stryker runs with `GUARDRAILS_MUTATION=1`: exclude slow integration suites in your Vitest config when it's set, as this repo does.

## 12. Suite health [SUITE-1..6]
`guardrails check-tests` audits the whole suite (ported from nexus-core's scanner):
- No `.only`, no unexplained skips (a reason, issue or `TEST-SKIP:` marker beside it), and no reading product source as text to assert on it [SUITE-1].
- Test files stay at or under 300 lines (`testSuite.maxLines`) [SUITE-2]. No fixed sleeps [SUITE-3]. No real clock inside an assertion without fake timers [SUITE-4]. No copy-paste test bodies or files that repeat each other's titles [SUITE-5].
- It reports the test/product line ratio per package.

Existing findings live in `.guardrails/test-suite-baseline.json`, so only regressions block. `--update-baseline` only ratchets down. Growing the baseline needs the rebaseline label [SUITE-6]. Large full suites can shard with `guardrails shard --shard i/n --run "<runner>"`, or `vitest run --shard=i/n` (see template/.github/workflows/full-suite-sharded.yml).
