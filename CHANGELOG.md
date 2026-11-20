# Changelog

All notable changes are documented here, following Keep a Changelog.
Version: 0.6.3 · Date: 2026-11-20

## [0.6.3] - 2026-11-20

### Fixed
- `guardrails check --fails-on-base` now unlinks the base worktree's `node_modules` link before removing the worktree, so cleanup no longer deletes the checkout's real `node_modules` directory (Windows junctions and POSIX symlinks).

## [0.6.1] - 2026-11-19

### Changed
- Every approval label now follows `guardrails:<what it approves>-approved`, so a code owner can see what they are signing off:

  | Old label | New label | Approves |
  |---|---|---|
  | `guardrails:config-approved` (for journeys) | `guardrails:journeys-approved` | The journey changes in the PR (JRN-3) |
  | `guardrails:config-approved` (for config) | `guardrails:config-approved` (unchanged) | Loosening `guardrails.config.json` (TEST-7 config part) |
  | `tests-removed-approved` | `guardrails:tests-removed-approved` | Deleting base-branch tests (TEST-7) |
  | `allow-characterization` | `guardrails:existing-behaviour-tests-approved` | New tests that lock in existing behaviour and already pass on base (TEST-11) |
  | `guardrails:rebaseline` | `guardrails:baseline-approved` | The baseline may grow (BASE-1, SUITE-6) |
  | `flags-resolved` | `guardrails:flags-approved` | Open flags stop blocking (FLAG-1) |

- Error and fix messages now say plainly what the code owner is approving.
- New config key `labels.journeysApproved` (default `guardrails:journeys-approved`).

### Deprecated
- The old label names still work in this release as aliases and print a warning on stderr naming the new label. `guardrails:config-approved` still approves journeys (JRN-3) with a warning. The aliases will be removed in a later release.

### Migration
1. Create the new labels in each repository that uses guardrails (labels are never created for you).
2. Replace the old labels on open pull requests, then delete the old labels.
3. If you set custom names under `labels` or `tests.testsRemovedLabel`, nothing changes for those keys, except that `labels.journeysApproved` is new: a journeys change now needs it, or the deprecated `guardrails:config-approved` alias.

## [0.6.0] - 2026-11-19

### Added
- Codex CLI support: `init` writes `.codex/config.toml` (MCP server and `GUARDRAILS_AGENT=codex` for Codex's shell) and `.codex/hooks.json`; `guardrails hook-guard --agent codex` guards shell commands and `apply_patch`. Existing `config.toml` files are only appended to.
- The hook locks `.codex/hooks.json` and `.codex/config.toml` by default and finds the project root from a subfolder.

### Fixed
- A fresh `init` followed by `check` now passes: the template no longer enables the runbook and CI standards that `init` does not scaffold. `init runbook` still opts in.
- `journeys draft --dry-run` never calls Gemini or OpenAI; it previews the route heuristic.
- The MCP server reports the package version instead of a hardcoded `0.1.0`.
- RULES.md lists every never-baselined rule (SUITE-1 to 6 were missing) and JRN-5 comes before JRN-6.
- README and CLI help state the real journeys draft count (up to `journeys.draft.max`, default 8, at most 20).

### Removed
- `holdout.repo` (it was never read; `holdout.path` and `guardrails holdout --dir` are how a checkout is supplied).

### Changed
- Package renamed to `@ws42ca/guardrails` (first public npm name).
- TEST-9 accepts both `@ws42ca/guardrails/vitest-setup` and the legacy `@ws42/guardrails/vitest-setup`.
- The init template workflow (`template/.github/workflows/guardrails.yml`) now runs the installed package instead of a reusable workflow in a private repository.

## [0.5.0] - 2026-11-15

### Added
- Suite auditing with `guardrails check-tests` (SUITE-1..6), a committed baseline that blocks regressions, and test/product ratio reporting.
- Optional shard helper and sharded full-suite workflow; suite policy in TESTING.md.

## [0.4.x] - 2026-11-15

### Added
- Change-scoped CI (CI-1), `guardrails affected`, `test:changed` and `test:full`, with nightly and predeploy full-suite safety nets.

### Changed
- Clock-shift checks moved off pull requests; affected runs stream runner output.

### Fixed
- Affected selection includes tests that name changed files and respects CI test scope.
- Named-file runner commands and `test-*` helpers are recognised as focused runs.

## [0.3.x] - 2026-11-15

### Added
- Runbook standard (RUN-1..4), `guardrails init runbook`, smoke and secrets-check helpers.
- Top-level documentation allowlist and runtime documentation globs.

### Fixed
- Deploy detection ignores dry-run builds and recognises Node-invoked Wrangler deploys.
- Stray runbook detection excludes tickets merely mentioning deployment.

## [0.2.0] - 2026-11-15

### Added
- Anti-cheating checks: tests fail on base, test-gaming detection, stop-and-flag, test locks, hidden holdouts, duplication, npm registry checks and secrets scanning.

## [0.1.0] - 2026-11-15

### Added
- TypeScript package scaffold, core checks, rules catalogue, strict configuration, baseline ratchet and reports.
- Guardrails command-line interface and stdio Model Context Protocol (MCP) server.
- README, testing policy, agent skill, repository template and reusable workflow.
- Assertion setup enforcement, CODEOWNERS protection and test-count drop warnings.
