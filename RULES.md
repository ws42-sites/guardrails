# Rules catalogue

Every finding names one of these IDs. `guardrails explain <ID>` (or the MCP tool `guardrails_explain_rule`) prints the same text. Override a mode per project in `guardrails.config.json` → `rules` (lowering a mode counts as weakening, TEST-7). "(diff)" rules only run with `--base`.

| ID | Default | Rule | Why | Fix |
|---|---|---|---|---|
| CFG-1 | block | Config is valid | Unknown or misspelt keys would silently disable a rule. | Fix guardrails.config.json so it matches schema/guardrails.schema.json. |
| ARCH-1 | block | Declared import boundaries | Agents copy whatever import path works; boundaries make the wrong one impossible. | Import through the allowed layer (for example the storage interface), not across the forbidden edge. |
| ARCH-2 | warn | Single implementation per capability | Copies of the same helper drift; one copy ends up untested or unused. | Delete the duplicate and import the one shared implementation. |
| ARCH-3 | warn | Production reachability | Code only tests exercise looks covered but never runs in production. | Wire the module into a production entry, or delete it (and its tests). |
| ARCH-4 | block (diff) | No unapproved new dependencies | Adding a library is the usual workaround for a design problem. | Remove the dependency. To keep it, a code owner adds it to deps.allow in guardrails.config.json (a CODEOWNER-protected change). |
| ARCH-5 | block | Restricted strings | Raw calls to a backend (for example api.github.com) bypass the layer that owns it. | Move the call into the allowed module and call that module instead. |
| ARCH-6 | block | No fixtures in production | Demo data shipped to every user (a hard-coded fixture image) is a real bug. | Only tests and dev-only code may reference fixtures. Load real data through the app. |
| TEST-1 | block | Spec IDs resolve | Every test must map to a promised behaviour; untagged or unknown tags are padding. | Start the test (or its describe) title with a spec ID from the specs block, e.g. "REVIEW-03 expired token returns 410". |
| TEST-2 | block | High-tier specs are tested | Risky behaviours need at least one test; everything else is optional. | Add one focused test for the spec ID, or remove the spec if it is no longer a behaviour. |
| TEST-3 | warn (diff) | Test budget | Test lines far above product lines usually means padding. | Merge cases into it.each tables and drop tests that do not map to a distinct behaviour. |
| TEST-4 | warn | Test file size and tests per spec | Huge test files and many tests per spec are where duplication hides. | Split by behaviour, use it.each, or delete redundant cases. |
| TEST-5 | block | Banned test patterns | Focused/skipped tests, snapshots in T1, real clocks, randomness, fake servers and Stryker-disable comments make tests lie. | Remove .only/.skip/todo, use vi.useFakeTimers() + vi.setSystemTime(), fixed inputs instead of random loops, explicit assertions instead of snapshots. |
| TEST-5W | warn | Weak assertions (heuristic) | Tests that only assert a mock was called, or only toBeDefined/toBeTruthy, rarely catch bugs. | Assert on the observable result (returned value, written data, response status). |
| TEST-6 | block | Clock independence | Tests that depend on today's date break later (the Nov 30 time bomb). | Freeze time with vi.useFakeTimers() and vi.setSystemTime(new Date("2026-01-01")) in the failing test. |
| TEST-7 | block (diff) | No test weakening | Deleting tests, removing expects, loosening matchers or lowering thresholds is the cheapest way to go green. | Restore the test/assertion/threshold. Deleting base-branch tests needs the tests-removed-approved label from a code owner; tests added in the same PR can be deleted freely. |
| TEST-8 | block | Mutation score | The real answer to "do these tests test anything". | Add assertions that would fail if the surviving mutants were real bugs. |
| DOC-1 | block | Doc allowlist | Agents create a new doc per task; they go stale and compete with the real instructions. | Put the content in an allowlisted doc (or a check), then delete the file. |
| DOC-2 | block | One agent instruction file | AGENTS.md is the single source of truth for every coding tool. | Make GEMINI.md/CLAUDE.md/etc. a one-line pointer to AGENTS.md. |
| DOC-3 | warn | Doc length cap | Long docs mean rules that should be checks. | Move rules into checks and trim the doc. |
| BASE-1 | block (diff) | Baseline only shrinks | Re-baselining new violations would switch the checks off. | Fix the violation instead. Growing the baseline needs the rebaseline label from a code owner. |

## Engines
- **ARCH-1, ARCH-6 (imports):** dependency-cruiser, with rules generated from `architecture.forbidden` and `fixtures.globs`. Test and helper files are never the "from" side.
- **ARCH-3:** knip `--production`, with entries from `architecture.entries` and tests ignored.
- **TEST-5:** ESLint (`eslint-plugin-vitest` plus `no-restricted-syntax` and two small custom rules). The project's own ESLint config is not used, so it can't be loosened from there.
- **TEST-1/2/4/5W/7:** a TypeScript AST inventory of every `it`/`test` (title, spec ID, expect count, matchers).
- **TEST-6:** reruns `tests.command` with `NODE_OPTIONS=--import clock-shift.mjs` and the clock at +`tests.clockShiftYears`. Fake timers still win.
- **TEST-8:** Stryker, using a generated config (pinned Stryker version, `mutation.excludedMutations`, incremental), run on changed files in `mutation.tiers`.

## Block vs warn
- Blocking from day one: CFG-1, ARCH-1, ARCH-4, ARCH-5, ARCH-6, TEST-1, TEST-2 (T1), TEST-5, TEST-6, TEST-7, DOC-1, DOC-2, BASE-1.
- Warn: ARCH-2, ARCH-3, TEST-2 (T2), TEST-4, TEST-5W, DOC-3. Promote them once the signal is clean.
- Always warn: TEST-3 (budget). It flags review attention and is never the reason a PR fails.

## Baseline
`guardrails baseline` records current violations in `.guardrails/baseline.json` as `{rule, file, fingerprint}`. The fingerprint never includes a line number. Known violations are reported but don't block. Re-running only removes fixed entries. `--force` re-records everything; CI blocks any growth (BASE-1) unless a code owner adds `guardrails:rebaseline`. Diff-only and runtime rules (TEST-3, TEST-6, TEST-7, TEST-8, ARCH-4, BASE-1, CFG-1) are never baselined.
