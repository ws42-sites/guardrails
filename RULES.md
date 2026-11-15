# Rules catalogue

Every finding names one of these IDs. `guardrails explain <ID>` (or the MCP tool `guardrails_explain_rule`) prints the same text. Override a mode per project in `guardrails.config.json` → `rules` (lowering a mode counts as weakening, TEST-7). "(diff)" rules only run with `--base`.

| ID | Default | Rule | Why | Fix |
|---|---|---|---|---|
| CFG-1 | block | Config is valid | Unknown or misspelt keys would silently disable a rule. | Fix guardrails.config.json so it matches schema/guardrails.schema.json. |
| CFG-2 | block | CODEOWNERS guards the guardrails | Without a human owner on the config, workflow and baseline, every rule is one edit away from off. | Add CODEOWNERS lines with an owner for guardrails.config.json, .github/workflows/ and the baseline file (see template/.github/CODEOWNERS). |
| ARCH-1 | block | Declared import boundaries | Agents copy whatever import path works; boundaries make the wrong one impossible. | Import through the allowed layer (for example the storage interface), not across the forbidden edge. |
| ARCH-2 | warn | Single implementation per capability | Copies of the same helper drift; one copy ends up untested or unused. | Delete the duplicate and import the one shared implementation. |
| ARCH-3 | warn | Production reachability | Code only tests exercise looks covered but never runs in production. | Wire the module into a production entry, or delete it (and its tests). |
| ARCH-4 | block (diff) | No unapproved or suspicious new dependencies | Adding a library is the usual workaround, and 3.6-5% of agent-suggested package names do not exist (slopsquatting). Young or rarely downloaded packages are a supply-chain risk. | Remove the dependency. To keep it, a code owner adds it to deps.allow in guardrails.config.json (a CODEOWNER-protected change). |
| ARCH-5 | block | Restricted strings | Raw calls to a backend (for example api.github.com) bypass the layer that owns it. | Move the call into the allowed module and call that module instead. |
| ARCH-6 | block | No fixtures in production | Demo data shipped to every user (a hard-coded fixture image) is a real bug. | Only tests and dev-only code may reference fixtures. Load real data through the app. |
| ARCH-7 | block | Test-gaming signatures in product code | Agents make tests pass by detecting the test runner, exiting early, overloading equality, counting calls or special-casing expected values (ImpossibleBench, METR). | Remove the special case and implement the behaviour. If the test looks wrong, call guardrails_flag instead. |
| ARCH-8 | block (diff) | Duplication on new code | Copy-paste in new code is how agents route around shared code (Sonar gate: <=3% on new code). | Extract the shared logic and call it from both places. |
| TEST-1 | block | Spec IDs resolve | Every test must map to a promised behaviour. Unknown or malformed IDs block (invented behaviours); a missing ID only warns. | Start the test (or its describe) title with a spec ID from the specs block, e.g. "REVIEW-03 expired token returns 410". |
| TEST-2 | block | High-tier specs are tested | Risky behaviours need at least one test; everything else is optional. | Add one focused test for the spec ID, or remove the spec if it is no longer a behaviour. |
| TEST-3 | warn (diff) | Test budget | Test lines far above product lines usually means padding. | Merge cases into it.each tables and drop tests that do not map to a distinct behaviour. |
| TEST-4 | warn | Test file size and tests per spec | Huge test files and many tests per spec are where duplication hides. | Split by behaviour, use it.each, or delete redundant cases. |
| TEST-5 | block | Banned test patterns | Focused/skipped/todo tests, real clocks, unseeded randomness and Stryker-disable comments make tests lie (block). Snapshots in T1, class/style assertions, fake servers, big loops and Date/Response spies are weak (warn). | Remove .only/.skip/todo, use vi.useFakeTimers() + vi.setSystemTime(), fixed inputs instead of random loops, explicit assertions instead of snapshots. |
| TEST-5W | warn | Weak assertions (heuristic) | Tests that only assert a mock was called, or only toBeDefined/toBeTruthy, rarely catch bugs. | Assert on the observable result (returned value, written data, response status). |
| TEST-6 | block | Clock independence | Tests that depend on today's date break later (the Nov 30 time bomb). | Freeze time with vi.useFakeTimers() and vi.setSystemTime(new Date("2026-01-01")) in the failing test. |
| TEST-7 | block (diff) | No test weakening | Deleting tests, removing expects, loosening matchers or lowering thresholds is the cheapest way to go green. | Restore the test/assertion/threshold. Deleting base-branch tests needs the tests-removed-approved label from a code owner; tests added in the same PR can be deleted freely. |
| TEST-9 | block | Every test must assert | A test with no assertion always passes. | Add "@ws42/guardrails/vitest-setup" (or a setup file that calls expect.hasAssertions() in beforeEach) to Vitest setupFiles. |
| TEST-10 | warn (diff) | Test count dropped | Fewer tests than the base branch deserves a reviewer look, even when each deletion is approved. | Check the deleted tests were redundant; the job summary lists them. |
| TEST-11 | block (diff) | New tests fail on base | A test should fail if you revert the implementation. A new test that already passes on the base branch proves nothing about this change. | Make the test assert the new behaviour. If it intentionally guards existing behaviour, mark the spec regressionGuard: true or get the allow-characterization label. |
| SCOPE-1 | warn (diff) | PR scope | Big or off-task diffs hide cheating and are hard to review. | Split the PR, or keep changes inside the paths of the task's spec IDs. |
| FLAG-1 | block | Open agent flags | Letting agents stop and flag an impossible or conflicting test cut cheating from 54% to 9% (ImpossibleBench); a human must answer before merge. | A human resolves the flag (fix the spec/test, then empty .guardrails/flags.json or add the flags-resolved label). |
| HOLD-1 | block | Hidden holdout tests | Tests the agent never sees cannot be gamed. | Implement the behaviour the spec describes; the holdout tests are not visible by design. |
| SEC-1 | block | Secret scanning | Agents paste keys into code and fixtures. | Remove the secret, rotate it, and load it from the environment. (Runs in CI via gitleaks.) |
| TEST-8 | block | Mutation score | The real answer to "do these tests test anything". | Add assertions that would fail if the surviving mutants were real bugs. |
| DOC-1 | warn | Doc allowlist | Agents create a new doc per task; they go stale and compete with the real instructions. | Put the content in an allowlisted doc (or a check), then delete the file. |
| DOC-2 | warn | One agent instruction file | AGENTS.md is the single source of truth for every coding tool. | Make GEMINI.md/CLAUDE.md/etc. a one-line pointer to AGENTS.md. |
| DOC-3 | warn | Doc length cap | Long docs mean rules that should be checks. | Move rules into checks and trim the doc. |
| BASE-1 | block (diff) | Baseline only shrinks | Re-baselining new violations would switch the checks off. | Fix the violation instead. Growing the baseline needs the rebaseline label from a code owner. |

## Engines
- **ARCH-1, ARCH-6 (imports):** dependency-cruiser, with rules generated from `architecture.forbidden` and `fixtures.globs`. Test and helper files are never the "from" side.
- **ARCH-3:** knip `--production`, with entries from `architecture.entries` and tests ignored.
- **ARCH-4:** `package.json` diff plus the npm registry for every new package (and new `deps.allow` entries): it must exist (else block), be older than `deps.registry.minAgeDays` and have `minWeeklyDownloads` (else warn), and have no high/critical advisory (block). Results are cached in `node_modules/.cache/guardrails/`. `GUARDRAILS_OFFLINE=1` or no network skips the lookup with a metric.
- **ARCH-7:** line scans of product code (test-runner checks, `process.exit` outside `architecture.cliEntries`) and test files (`expect.extend`, `vi.mock` of the module under test); in diff mode, added `valueOf`/`equals`/`Symbol.toPrimitive` (block) and added call counters, cheat comments and literals shared with new test expectations (warn).
- **ARCH-8:** jscpd (`duplication.minLines`/`minTokens`) over product files, counted only on lines the PR adds.
- **TEST-5:** ESLint (`eslint-plugin-vitest` plus `no-restricted-syntax` and two small custom rules). The project's own ESLint config is not used, so it can't be loosened from there.
- **TEST-1/2/4/5W/7:** a TypeScript AST inventory of every `it`/`test` (title, spec ID, expect count, matchers, source hash).
- **TEST-6:** reruns `tests.command` with `NODE_OPTIONS=--import clock-shift.mjs` and the clock at +`tests.clockShiftYears`. Fake timers still win.
- **TEST-11:** `git worktree` of the merge-base, the PR's changed test and helper files copied in, `tests.command` run on just those files (JSON reporter) there and on head. New or changed tests must fail on base and pass on head.
- **TEST-9, CFG-2:** read Vitest `setupFiles` and CODEOWNERS (last matching line wins, like GitHub).
- **TEST-8:** Stryker, using a generated config (pinned Stryker version, `mutation.excludedMutations`, incremental), run on changed files in `mutation.tiers`. Runs with `GUARDRAILS_MUTATION=1` so a project's Vitest config can exclude slow suites. The gate always recomputes the score; `guardrails_mutants` only lists survivors for the agent.
- **FLAG-1:** `.guardrails/flags.json` (written by `guardrails flag` / MCP `guardrails_flag`).
- **HOLD-1:** `guardrails holdout --dir <checkout>` in the reusable workflow's `holdout` job.
- **SEC-1:** gitleaks (pinned CLI image) in the reusable workflow's `secrets` job, over the PR's commits. CodeQL is an optional job (`codeql: true`).
- **SCOPE-1:** diff line counts plus `specs.items.<ID>.paths` for the task's spec IDs (`--specs`, `$GUARDRAILS_SPECS`, or the IDs in changed tests).

## Block vs warn
BLOCK is kept for correctness, security and cheating. Preference-style rules warn.
- Block: CFG-1, CFG-2, ARCH-1, ARCH-4 (missing package, unapproved, high advisory), ARCH-5, ARCH-6, ARCH-7 (block signatures), ARCH-8, TEST-1 (unknown/malformed ID), TEST-2 (T1), TEST-5 (`.only`/`.skip`/todo/empty each, real clock, unseeded randomness, Stryker-disable), TEST-6, TEST-7, TEST-8, TEST-9, TEST-11, FLAG-1, HOLD-1, SEC-1, BASE-1.
- Warn: ARCH-2, ARCH-3, ARCH-4 (young/low-download package, moderate advisory), ARCH-7 (heuristics), TEST-1 (missing ID), TEST-2 (T2), TEST-3, TEST-4, TEST-5 (T1 snapshots, class/style assertions, fake servers, big loops, Date/Response/prototype spies, seeded property runs outside `properties` specs), TEST-5W, TEST-10, SCOPE-1, DOC-1, DOC-2, DOC-3.
- Moved from block to warn in v0.2: DOC-1, DOC-2, TEST-1 missing ID, and the TEST-5 preference bans listed above.

## Labels (set by a human on the PR)
- `tests-removed-approved`: base tests may be deleted (TEST-7).
- `guardrails:rebaseline`: the baseline may grow (BASE-1).
- `allow-characterization`: new tests may pass on base (TEST-11). Per spec, `regressionGuard: true` does the same.
- `flags-resolved`: open flags no longer block (FLAG-1). Emptying `.guardrails/flags.json` also clears them.
- `guardrails:config-approved`: a code owner approved loosening `guardrails.config.json` (TEST-7 config part), for example a new `deps.allow` entry.

## Baseline
`guardrails baseline` records current violations in `.guardrails/baseline.json` as `{rule, file, fingerprint}`. The fingerprint never includes a line number. Known violations are reported but don't block. Re-running only removes fixed entries. `--force` re-records everything; CI blocks any growth (BASE-1) unless a code owner adds `guardrails:rebaseline`. Diff-only and runtime rules (TEST-3, TEST-6, TEST-7, TEST-8, TEST-9, TEST-10, TEST-11, ARCH-4, ARCH-8, SCOPE-1, FLAG-1, HOLD-1, SEC-1, BASE-1, CFG-1, CFG-2) are never baselined.
