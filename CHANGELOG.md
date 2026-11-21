# Changelog

All notable changes are documented here, following Keep a Changelog.
Version: 0.7.0 · Date: 2026-11-21

## [Unreleased]

### Fixed
- File listing skips generated directories (`node_modules`, `dist`, and the rest of the skip list) even when `git ls-files` would show them. `guardrails init` creates `.gitignore`, or appends the missing lines. A fresh install no longer baselines dependency files or fails TEST-9 because of them.

### Changed
- README states that 0.7.0 has shipped, shows a check that blocks a loosened matcher, and lists what Guardrails does not do.
- Development dependencies: Vitest `^4.1.12` (Node 20) and Stryker 9.6.1. CI checks the README rule count and host list, rejects "planned" wording for released versions, treats DOC-1 as a failure, smoke-tests the quickstart from `npm pack`, and fails `npm audit` on high or critical advisories other than the documented Zod 3 exception.
- Tag publishes fail when `NPM_TOKEN` is missing instead of skipping, and the GitHub release notes are the changelog section for that version.

## [0.7.0] - 2026-11-21

Targeted as `0.7.0` rather than `0.6.x` because `0.6.3` is already on npm and this release adds opt-in rules, commands and config keys. Everything new is off by default, so a basic install behaves like `0.6.3`.

### Added
- Independent reviewer (`guardrails review`, `guardrails check --review`, rule REVIEW-1). Sees only the merge-base diff and check output—never author reasoning, PR descriptions or commit messages—via a typed provider (`stub` or `command`), fails closed, runs on risky paths by default (`review.onlyWhenRisky`), and shards with `--shard i/n`. Off unless `review.enabled` is true.
- Commit-pinned provenance (`guardrails provenance record --report <file>`, `guardrails provenance verify --head <sha>`, rule PROV-1). Records tested SHA, commit parents, report sha256, guardrails version, checks and override labels (who, when, why) as JSON plus a `<!-- guardrails:provenance v1 ... -->` marker, and fails verification on an untested head or altered report. Off unless `provenance.enabled` is true.
- Checker evaluation (`guardrails ledger add|import`, `guardrails evaluate`): offline `.guardrails/ledger.jsonl` tracking `escaped-bug` and `false-escalation` evidence, with cached weekly soften/prune/sharpen/add proposals that are never auto-applied (`examples/guardrails-weekly.yml`). Off unless `evaluation.enabled` is true.
- Feedback loop (`guardrails feedback draft`): tags blocking findings with `{rule, file, agent}` (`GUARDRAILS_AGENT_NAME` or `--agent-name`, default `unknown`) into `feedback.logFile` and clusters repeats into candidate lint rules for human review. Off unless `feedback.enabled` is true.
- Label planner (`guardrails labels plan [--base ref] [--labels a,b] [--format markdown|json]`): statically scans the diff and open flags to summarise needed (and already present) approval labels with `file:line`. Advisory only (always exits `0`, never adds labels).
- `guardrails.md` config format (`template/guardrails.md`): optional `dotted.key: value` front matter and fenced `guardrails` / `json guardrails` JSON blocks merged below `guardrails.config.json`. Locked by agent hooks, covered by CFG-2 and TEST-7 weakening checks, copied into `--fails-on-base` worktrees, and exempt from DOC-1 at the repo root.
- Opt-in anonymous telemetry (`telemetry.enabled` + `telemetry.endpoint`; disabled by `GUARDRAILS_TELEMETRY=0`, and in `CI` also requires `GUARDRAILS_TELEMETRY=1`) plus `guardrails telemetry status`.
- `guardrails report --report <file>` re-renders a saved JSON report as text, markdown or JSON without re-running the checks.
- Reusable workflow inputs `label-plan` (default `true`, advisory), `review`, `review-shards`, `provenance` (default `false`) and `mutation-shards` (default `'["1/1"]'`, saving per-shard Stryker incremental caches even on failure), plus `scripts/ci-weight.mjs`.

### Changed
- In this repo's CI, pushes to `main` mutation-test only changed files, PR and full mutation runs shard 6 ways (`mutation-shards`), and full-repo mutation runs only on nightly and manual triggers.
- `guardrails.md` is included in the default `lock.paths`, and `hook-guard` resolves project roots from either `guardrails.config.json` or `guardrails.md`.
- `guardrails --help`, `-h` and `<command> --help` print usage to stdout and exit `0`. Usage errors, unknown commands and unknown `explain` rule IDs print to stderr and exit `2`. Blocking findings still exit `1`.
- Journey and config Zod schemas in `src/config.ts` are built lazily on first use, so mutation testing of that file no longer reconstructs the schemas for every mutant.
- README five-minute quickstart, exit-code table, TESTING.md and the agent skill now match current CLI behaviour and the 0.7.0 plan.

### Fixed
- `TEST-7` pairs base and head tests that share a title by position, so unchanged `it.each` or duplicate-title test families are no longer misreported as weakened.

## [0.6.3] - 2026-11-20

### Fixed
- `guardrails check --fails-on-base` now unlinks the base worktree's `node_modules` link before removing the worktree, so cleanup no longer deletes the checkout's real `node_modules` directory (Windows junctions and POSIX symlinks).

## [0.6.2] - 2026-11-19

### Added
- `guardrails init` now works in a repository that already has agent instruction files, so guardrails can be brought in partway through a project. Nothing outside the guardrails markers is ever changed, and a second run adds nothing.
  - An existing `AGENTS.md` gets the guardrails rules appended between `<!-- guardrails:start -->` and `<!-- guardrails:end -->`. A later run replaces only what sits between those markers.
  - An existing `CLAUDE.md` or `GEMINI.md` gets a one-line pointer to `AGENTS.md`, inside the same markers, unless it already mentions `AGENTS.md` or is a symlink to it.
  - An existing `.github/CODEOWNERS` gets the guardrails owner lines (config, `.guardrails/`, workflows, CODEOWNERS, `AGENTS.md`) appended only where that path has no line yet.
  - A file whose markers are broken (a start without an end) is left alone and reported as skipped.
  - Line endings of the existing file (LF or CRLF) are kept.
- `init` output says whether each file was `created`, `merged` or `skipped`. The library `init()` result has a new `merged` list: the files in `written` that were added into an existing file.

### Changed
- The `AGENTS.md` template wraps its Guardrails section in the same markers, so a file created by `init` can be updated by a later run.
- `--force` is unchanged: it overwrites the whole file.
- `CLAUDE.md` and `GEMINI.md` must stay a one-line pointer (DOC-2). If yours holds other content, `check` still asks you to move it into `AGENTS.md`.

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
