# Changelog

All notable changes are documented here, following Keep a Changelog.
Version: 0.5.0 · Date: 2026-11-15

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
