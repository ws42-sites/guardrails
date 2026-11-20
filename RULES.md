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
| TEST-7 | block (diff) | No test weakening | Deleting tests, removing expects, loosening matchers or lowering thresholds is the cheapest way to go green. | Restore the test/assertion/threshold. Deleting base-branch tests needs a code owner to add the guardrails:tests-removed-approved label; tests added in the same PR can be deleted freely. |
| TEST-9 | block | Every test must assert | A test with no assertion always passes. | Add "@ws42ca/guardrails/vitest-setup" (or a setup file that calls expect.hasAssertions() in beforeEach) to Vitest setupFiles. |
| TEST-10 | warn (diff) | Test count dropped | Fewer tests than the base branch deserves a reviewer look, even when each deletion is approved. | Check the deleted tests were redundant; the job summary lists them. |
| TEST-11 | block (diff) | New tests fail on base | A test should fail if you revert the implementation. A new test that already passes on the base branch proves nothing about this change. | Make the test assert the new behaviour. If it intentionally guards existing behaviour, mark the spec regressionGuard: true or have a code owner add the guardrails:existing-behaviour-tests-approved label (it approves new tests that lock in existing behaviour). |
| JRN-1 | block | Enforced journeys have one e2e test | Critical User Journeys (Google eval practice) are what users actually do; unit tests can all pass while sign-in is broken. Approved or client-provided P0 journeys block, P1 warn; unapproved inferred/template journeys only warn. | Write exactly one e2e test (Playwright, journeys.e2eGlobs) whose title starts with the journey ID and walks its steps. |
| JRN-2 | warn | Every spec serves an approved journey | A spec no journey needs signals padding or a feature nobody asked for. | Add the spec ID to an approved journey's specs (a human change), or drop the spec and its tests. |
| JRN-3 | block (diff) | Journeys are human-owned | Approved and client-provided journeys are protected. Base-trusted producers may add unapproved journeys with provenance and valid run steps, or regenerate unapproved non-client journeys. Trust and matching settings remain human-owned. | A code owner reviews the journeys change and adds the guardrails:journeys-approved label; approve with "guardrails journeys approve <id>". |
| JRN-4 | warn (diff) | New routes and forms belong to a journey | New pages, forms and public endpoints nobody mapped to a journey are untested surface or scope creep. | Ask the owner to add the route to a journey's routes, or confirm it is internal. |
| JRN-5 | warn | Journey visual diff (stub) | Per-step screenshots compared to the mocks catch layout drift (VIS-1 v2). Off by default; not implemented yet. | Leave journeys.visual.enabled false until VIS-1 v2 ships. |
| JRN-6 | block | Generated journey tests are current | Edited or stale generated tests no longer prove the declared journey. Unapproved inferred/template journeys only warn. | Regenerate with guardrails journeys generate-tests. |
| SCOPE-1 | warn (diff) | PR scope | Big or off-task diffs hide cheating and are hard to review. | Split the PR, or keep changes inside the paths of the task's spec IDs. |
| FLAG-1 | block | Open agent flags | Letting agents stop and flag an impossible or conflicting test cut cheating from 54% to 9% (ImpossibleBench); a human must answer before merge. | A human resolves the flag (fix the spec/test, then empty .guardrails/flags.json or have a code owner add the guardrails:flags-approved label). |
| HOLD-1 | block | Hidden holdout tests | Tests the agent never sees cannot be gamed. | Implement the behaviour the spec describes; the holdout tests are not visible by design. |
| SEC-1 | block | Secret scanning | Agents paste keys into code and fixtures. | Remove the secret, rotate it, and load it from the environment. (Runs in CI via gitleaks.) |
| TEST-8 | block | Mutation score | The real answer to "do these tests test anything". | Add assertions that would fail if the surviving mutants were real bugs. |
| DOC-1 | warn | Doc allowlist | Agents create a new doc per task; they go stale and compete with the real instructions. | Put the content in an allowlisted doc (or a check), then delete the file. |
| DOC-2 | warn | One agent instruction file | AGENTS.md is the single source of truth for every coding tool. | Make GEMINI.md/CLAUDE.md/etc. a one-line pointer to AGENTS.md. |
| DOC-3 | warn | Doc length cap | Long docs mean rules that should be checks. | Move rules into checks and trim the doc. |
| RUN-1 | block | RUNBOOK.md has the standard sections | Deploys drift when every repo documents them differently, and click-paths cannot be repeated by an agent or checked. | Run "guardrails init runbook". Keep the sections Deploy, Rollback, Smoke, Secrets rotation, Incident first steps in that order, each pointing at an `npm run <script>` (or saying n/a when runbook.na lists the script). |
| RUN-2 | block | Runbook commands exist | A runbook that names a missing script fails exactly when it is needed. | Add the script to package.json (deploy, deploy:preview, rollback, smoke, secrets:check) or mark it n/a with a reason in runbook.na. |
| RUN-3 | block | Deploy workflows use the scripts and smoke | Inline deploy commands in CI drift from the runbook; a deploy nobody smoke-tests is not done. | Call `npm run deploy` (or deploy:preview) in the workflow instead of the raw command, and run `npm run smoke` after it. |
| RUN-4 | block | One short runbook | Long or scattered runbooks go stale; one short file next to the scripts stays true. | Trim RUNBOOK.md under runbook.maxLines and fold other runbook/deploy docs into it, then delete them. |
| CI-1 | warn | Change-scoped CI with full-suite safety nets | Running every test on every PR is slow and expensive; affected tests catch the change, and a nightly and pre-deploy full run catches the rest. | On PRs run `npm run test:changed` (vitest --changed, `guardrails affected`, or the repo selector). Run `npm run test:full` on a nightly cron and in `predeploy`/`predeploy:preview`. |
| SUITE-1 | block | Banned test patterns (vs baseline) | .only hides the rest of the suite, unexplained skips rot, and tests that read product source as text break on every refactor without catching bugs. | Remove .only; give a skip a reason, issue or TEST-SKIP marker (or delete it); assert on behaviour instead of source text. Existing findings live in the test-suite baseline. |
| SUITE-2 | block | Test file line cap (vs baseline) | Test files over the cap (300 lines) are where duplication and slow setup hide. | Split the file by behaviour or fold cases into a table. Files already over the cap may shrink, not grow. |
| SUITE-3 | block | No fixed sleeps (vs baseline) | A fixed wait is slow when the event is fast and flaky when it is slow. | Wait for the event (a promise, expect.poll, waitFor, fake timers) instead of sleeping a fixed time. |
| SUITE-4 | block | No real clock in assertions (vs baseline) | Asserting against Date.now() or new Date() makes the result depend on when the suite runs. | Install fake timers and set the system time, or inject a clock. |
| SUITE-5 | block | No duplicate test families (vs baseline) | Copy-paste test bodies and files that repeat each other's titles cost CI time and test one behaviour many times. | Write one table-driven test, or merge the files so each behaviour is tested once. |
| SUITE-6 | block | Test-suite baseline only shrinks | The baseline records existing findings so only regressions block; growing it switches the checks off. The report also tracks the test/product line ratio. | Fix the new findings. "guardrails check-tests --update-baseline" only ratchets down; growing it needs a code owner to add the guardrails:baseline-approved label. |
| REVIEW-1 | block (diff) | Independent reviewer verdict | An author that grades its own work passes its own mistakes. A separate reviewer that sees only the diff and the test output, never the author's reasoning, PR description or commit messages, catches what the author talked itself into. | Fix the findings the reviewer reported in the code, not in the PR text. The reviewer is off by default; turn it on with review.enabled. |
| PROV-1 | block (diff) | Tested commit matches the PR head | A pass recorded for one commit says nothing about a later force-push or an untested head; the proof must be pinned to the exact commit that was tested. | Re-run the checks on the current head so a fresh provenance record pins its SHA. Provenance is off by default; turn it on with provenance.enabled. |
| BASE-1 | block (diff) | Baseline only shrinks | Re-baselining new violations would switch the checks off. | Fix the violation instead. Growing the baseline needs a code owner to add the guardrails:baseline-approved label. |

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
- **JRN-1..5:** the `journeys` block in `guardrails.config.json`. JRN-1 scans `journeys.e2eGlobs` (Playwright) for `test()`/`test.describe()` titles starting with a journey ID and reads pass/fail from `journeys.resultsFile` (Playwright JSON reporter) when set. JRN-3 diffs the journeys block against the merge-base. JRN-4 derives routes from new files under `journeys.routeGlobs` (pages, app router, Pages Functions) and from added `<Route path>`, `path:` and `app.get('/x')` lines, plus added `<form>`s. JRN-5 is a stub (VIS-1 v2).
- **SCOPE-1:** diff line counts plus `specs.items.<ID>.paths` for the task's spec IDs (`--specs`, `$GUARDRAILS_SPECS`, or the IDs in changed tests).

## Block vs warn
BLOCK is kept for correctness, security and cheating. Preference-style rules warn.
- Block: CFG-1, CFG-2, ARCH-1, ARCH-4 (missing package, unapproved, high advisory), ARCH-5, ARCH-6, ARCH-7 (block signatures), ARCH-8, TEST-1 (unknown/malformed ID), TEST-2 (T1), TEST-5 (`.only`/`.skip`/todo/empty each, real clock, unseeded randomness, Stryker-disable), TEST-6, TEST-7, TEST-8, TEST-9, TEST-11, FLAG-1, HOLD-1, SEC-1, JRN-1 (P0), JRN-3, BASE-1.
- Warn: ARCH-2, ARCH-3, ARCH-4 (young/low-download package, moderate advisory), ARCH-7 (heuristics), TEST-1 (missing ID), TEST-2 (T2), TEST-3, TEST-4, TEST-5 (T1 snapshots, class/style assertions, fake servers, big loops, Date/Response/prototype spies, seeded property runs outside `properties` specs), TEST-5W, TEST-10, SCOPE-1, JRN-1 (P1), JRN-2, JRN-4, JRN-5, DOC-1, DOC-2, DOC-3.
- Moved from block to warn in v0.2: DOC-1, DOC-2, TEST-1 missing ID, and the TEST-5 preference bans listed above.

## Critical User Journeys
A Critical User Journey (CUJ) is a complete task a user does. Journeys → specs → tests. Each journey retains its title, priority, prose steps, specs and routes; optional persona, goal and success describe intent. **Provenance** records its origin: client-provided, inferred or template. A source records by (producer), optional runId and inputs. Source without provenance defaults to inferred; legacy human journeys without source retain their behaviour.

| Journey | Merge policy |
|---|---|
| Approved, or client-provided | Enforced: P0 blocks, P1 warns; exactly one end-to-end (e2e) test; P2 coverage optional |
| Unapproved inferred or template | Missing, duplicate or failing tests only warn until a human approves |
| Unapproved without provenance | Draft; no JRN-1 violation |

The optional run list contains at most 40 declarative steps: goto, click, fill, submit, expectText, expectUrl, expectVisible, expectHref, expectNoHorizontalScroll, viewport and mockRoute. Each step has exactly one operation key. No JavaScript is accepted. Selectors are confined to submit and expectHref. Tokens such as {{client.phone}} are accepted in prose and run values; resolveJourney resolves them from client data and reports all unresolved or empty tokens before execution. Failures on a live site are alerts only; live monitoring is outside this merge-check policy.

**Trusted producers** are source.by names in journeys.trustedProducers (default []). JRN-3 reads trust from the base branch, never the proposed config. Without a label they may add unapproved journeys with provenance and valid run steps, or change/remove their unapproved non-client journeys. Approved/client-provided journeys, promotion to approval or client-provided status, and edits to trustedProducers, idPattern or e2eGlobs require guardrails:config-approved. A PR cannot trust itself.

Use guardrails journeys import <file.json> for an array of trusted producer journeys (each includes id). The entire batch is validated before writing; repeated imports are unchanged. Imports never approve, accept approvedBy, or overwrite protected journeys. Exact replays of client-provided journeys are permitted. Import works for agents; journeys approve <ID> remains human-only. Specs may list journeys as a back-reference; the schema checks both sides agree.

## Executable journeys

`guardrails journeys scaffold` creates missing Playwright configuration; `guardrails journeys generate-tests` resolves tokens from `journeys.dataFile` (default `.guardrails/client-data.json`) into portable specs and one shared runtime under `e2e/cuj`. `--check` is read-only. JRN-6 compares exact generated bytes and hashes: enforced journeys block; unapproved inferred/template journeys warn. Run with `guardrails journeys run --base-url <url>`; only enforced journey failures exit 1, while setup/report errors exit 2. Unmocked writes are aborted; declare `mockRoute` for every write endpoint.

## Labels (set by a human on the PR)
Every label is `guardrails:<what it approves>-approved`. Only a code owner should be able to add one.
- `guardrails:journeys-approved`: a code owner approved the journey changes in the PR (JRN-3).
- `guardrails:config-approved`: a code owner approved loosening `guardrails.config.json` (TEST-7 config part), for example a new `deps.allow` entry or a lower rule mode.
- `guardrails:tests-removed-approved`: base tests may be deleted (TEST-7).
- `guardrails:existing-behaviour-tests-approved`: new tests that lock in existing behaviour may already pass on base (TEST-11). Per spec, `regressionGuard: true` does the same.
- `guardrails:baseline-approved`: the baseline may grow (BASE-1, SUITE-6).
- `guardrails:flags-approved`: open flags no longer block (FLAG-1). Emptying `.guardrails/flags.json` also clears them.

Deprecated aliases (0.6.1 only, each prints a warning naming the new label): `guardrails:config-approved` still approves journeys (JRN-3), `tests-removed-approved`, `allow-characterization`, `guardrails:rebaseline` and `flags-resolved`.

## Baseline
`guardrails baseline` records current violations in `.guardrails/baseline.json` as `{rule, file, fingerprint}`. The fingerprint never includes a line number. Known violations are reported but don't block. Re-running only removes fixed entries. `--force` re-records everything; CI blocks any growth (BASE-1) unless a code owner adds `guardrails:baseline-approved`. Diff-only and runtime rules (TEST-3, TEST-6, TEST-7, TEST-8, TEST-9, TEST-10, TEST-11, ARCH-4, ARCH-8, SCOPE-1, FLAG-1, HOLD-1, SEC-1, JRN-3, JRN-4, JRN-5, BASE-1, CFG-1, CFG-2, SUITE-1, SUITE-2, SUITE-3, SUITE-4, SUITE-5, SUITE-6) are never baselined. SUITE-1 to 6 use their own test-suite baseline instead (`guardrails check-tests --update-baseline`).
