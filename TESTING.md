# Testing policy

Shared by every project that uses `@ws42/guardrails`. Projects don't copy this file; `AGENTS.md` links to it. Rule IDs in brackets are enforced (see RULES.md).

## 1. Tests come from specs, not from the code
- Behaviours live in `guardrails.config.json` → `specs.items`: an ID, a tier and one line saying what must hold. Example: `"REVIEW-02": { "tier": "T1", "behaviour": "Expired token returns 410 and writes nothing" }`.
- Every test title (or the `describe` around it) starts with a spec ID: `it("REVIEW-02 expired token returns 410", …)`. IDs match `^[A-Z][A-Z0-9]{1,11}-[0-9]{2,4}$` and must exist in the spec block [TEST-1].
- New behaviour means a new spec entry first. The config is CODEOWNER-protected, so a human approves what gets tested.
- More than 8 tests on one ID is padding. Use `it.each` [TEST-4].

## 2. Risk tiers decide how much testing
| Tier | What | Expectation |
|---|---|---|
| T1 | Public endpoints, auth, tokens, storage writes, money | Every T1 spec has a test [TEST-2]. Cover failure paths. Mutation score ≥ 70% on changed files [TEST-8]. No snapshots [TEST-5]. Budget ~1.0 test line per product line. |
| T2 | Internal logic | Happy path plus the edges that matter, as `it.each` tables. Untested T2 specs warn. Budget ~0.6. |
| T3 | UI layout | One flow test per page. Visuals are checked against the mocks by screenshot diff, never by class names or snapshots. Budget ~0.4. |

The budget [TEST-3] only warns. It points reviewers at padding; it never fails a PR on its own.

## 3. Banned patterns [TEST-5]
- `.only`, `.skip`, `xit`/`xdescribe`, `test.todo`, `each([])`.
- The real clock (`new Date()`, `Date.now()`, `performance.now()`, `process.hrtime`) unless the file uses `vi.useFakeTimers()`/`vi.setSystemTime()`.
- Randomness (`Math.random`, `crypto.randomUUID`, `getRandomValues`) unless seeded or mocked.
- Generated-input loops over 20 iterations, and fast-check without `numRuns ≤ 20`.
- Hand-rolled fake servers (`createServer`). Test through the storage interface or its fixture implementation.
- Spying on `Date`, `Response`, `crypto.subtle` or prototypes.
- `toHaveClass`/`toHaveStyle`. Snapshots in T1 tests.
- `// Stryker disable` comments anywhere.

Heuristics that only warn [TEST-5W]: tests with no `expect`, tests that only assert a mock was called, and tests whose only assertions are `toBeDefined`/`toBeTruthy`/`not.toThrow`/`expect(true)`. Every project must add `@ws42/guardrails/vitest-setup` (or its own `expect.hasAssertions()` setup) to `setupFiles` [TEST-9].

## 4. Time [TEST-6]
CI reruns the suite with the clock moved forward 2 years. A test that passes today and fails then is a time bomb, and it blocks. Freeze time in the test.

## 5. Never weaken a test [TEST-7]
In a PR, these block:
- deleting a test that exists on the base branch (needs the `tests-removed-approved` label from a code owner)
- removing an `expect`, or loosening a matcher (`toEqual` → `toMatchObject`/`toContain`, `toBe` → `toBeTruthy`)
- lowering any threshold, mode, tier or allowlist in `guardrails.config.json`, or a coverage/mutation threshold in Vitest or Stryker config

Tests added in the same PR can be changed or deleted freely. Moving a test to another file is not a deletion. Any drop in the total test count versus base warns with the count [TEST-10].

## 6. Size
Test files over 300 lines warn [TEST-4]. Split them by behaviour.

## 7. Mutation testing [TEST-8]
Stryker runs on changed files in `mutation.tiers` (T1 by default), incrementally, with pinned mutators. A surviving mutant means no test notices that bug. Add an assertion that would.
