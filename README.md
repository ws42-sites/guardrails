# @ws42ca/guardrails

**Guardrails that stop AI coding agents from padding, weakening or bypassing your tests, docs and architecture.**

[![npm](https://img.shields.io/npm/v/@ws42ca/guardrails)](https://www.npmjs.com/package/@ws42ca/guardrails)
![license](https://img.shields.io/npm/l/@ws42ca/guardrails)
[![dogfood: guardrails on itself](https://github.com/ws42-sites/guardrails/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/ws42-sites/guardrails/actions/workflows/ci.yml)

The badge is Guardrails' own CI: this repo runs every Guardrails check on itself (tests, diff rules, fails-on-base, mutation, secret scan) on each pull request.

Coding agents optimise for a green build. Left alone, they delete the failing test, loosen an assertion, special-case the expected value, add a package that does not exist, or write a new doc for every task. `guardrails` is a config-driven checker that blocks those shortcuts in CI and inside the agent's own editor. It ships as one CLI, one MCP server, an agent skill and one config file per project. Project page: [ws42.ca/guardrails](https://ws42.ca/guardrails)

## What it does

- **Keeps tests honest.** Every test maps to a spec ID, risky specs must have a test, and every test must assert something. Deleting tests, removing `expect`s, loosening matchers or lowering thresholds is blocked on pull requests. New tests must fail on the base branch so they prove the change. Optional mutation testing with Stryker checks that tests catch real bugs.
- **Catches test-gaming.** Product code that detects the test runner, exits early, overloads equality or hard-codes expected values is blocked (ARCH-7).
- **Protects architecture and dependencies.** Import boundaries (dependency-cruiser), production reachability (knip), duplication on new code (jscpd), restricted strings, no fixtures in production, and npm registry checks on new packages (existence, age, downloads, advisories).
- **Describes the product as journeys.** Critical user journeys (CUJs) are plain-language tasks that humans approve. Approved journeys need an end-to-end test, and the CLI can generate and run Playwright tests from declarative steps.
- **Locks the rules.** Agent hooks for Claude Code, Gemini CLI, Antigravity and Codex CLI deny edits to the config, baseline, CI files and spec-tagged tests. Agents stop and flag conflicts instead of bending tests (FLAG-1).
- **Keeps docs and ops tidy.** One `AGENTS.md` instruction file, a top-level doc allowlist and length cap, plus opt-in standards for `RUNBOOK.md`, change-scoped CI and test-suite health.

There are 49 rules. `npx guardrails explain` lists them, and `npx guardrails explain <ID>` prints why a rule exists and how to fix a finding. The same catalogue ships in the package as `RULES.md`.

## Five-minute quickstart

Requires **Node 20+** inside a **git repository** (for a brand-new folder, run `git init -b main` and ensure `package.json` and `src/` exist first). Optional peer dependencies for the Vitest setup and mutation testing: `npm i -D -E vitest @stryker-mutator/core@8.7.1 @stryker-mutator/vitest-runner@8.7.1`.

```bash
# 1. Install
npm i -D @ws42ca/guardrails

# 2. Scaffold config, AGENTS.md (+ CLAUDE.md/GEMINI.md pointers), CODEOWNERS, CI workflow and agent hooks
npx guardrails init --owner @your-github-handle --base main

# 3. If the repo already has test files, wire expect.hasAssertions() in vitest.config.ts (TEST-9 is never baselined):
#    import { defineConfig } from 'vitest/config';
#    export default defineConfig({ test: { setupFiles: ['@ws42ca/guardrails/vitest-setup'] } });

# 4. Record today's baselineable findings (the baseline only shrinks) and run every check
npx guardrails baseline
npx guardrails check

# 5. Commit the setup on main, then run diff rules on a feature branch
git add -A && git commit -m "chore: add guardrails"
git checkout -b my-change
npx guardrails check --base main --fails-on-base          # use --base origin/main once pushed to a remote
```

`init` never overwrites an existing file unless you pass `--force`. In an existing repo it merges between `<!-- guardrails:start -->` and `<!-- guardrails:end -->` markers in `AGENTS.md`, `CLAUDE.md` and `GEMINI.md`, merges hook/MCP settings, appends missing `.codex/config.toml` tables and missing `.github/CODEOWNERS` paths, and prints `created`, `merged` or `skipped` per file. A fresh `init` with no test files (or with `@ws42ca/guardrails/vitest-setup` configured) passes `npx guardrails check` out of the box; runbook, CI and suite standards stay off until `npx guardrails init runbook`.

### Exit codes

- **`0`** — Pass / ok (no unknown blocking findings; `report`, `labels plan`, `guardrails --help` and `<command> --help` also exit `0`).
- **`1`** — Blocking findings, or a failed check (`check`, `check-tests`, `check-links`, `smoke`, `secrets-check`, `review`, `provenance verify`, `holdout`, `journeys run`, or `validate-test`).
- **`2`** — Bad usage, unknown command, unknown rule ID in `explain <ID>`, or runtime/setup error (such as a missing required flag or an unknown `--base` git ref). In `hook-guard`, exit `2` denies a locked edit for Claude Code, Gemini CLI and Codex CLI.

Note: run `npx guardrails --help` or `npx guardrails <command> --help` for usage (both exit `0` and print to stdout).

## Configuration

Everything lives in `guardrails.config.json` (and optionally `guardrails.md`). Unknown keys fail CFG-1 so a typo cannot silently disable a rule.

```json
{
  "$schema": "./node_modules/@ws42ca/guardrails/schema/guardrails.schema.json",
  "version": 1,
  "baseBranch": "main",
  "sourceRoots": ["src"],
  "tiers": { "T1": ["src/payments/**"], "T2": ["src/lib/**"], "T3": ["src/**"] },
  "specs": {
    "items": {
      "ORDER-01": { "tier": "T1", "behaviour": "A paid order sends a receipt email" },
      "ORDER-02": { "tier": "T2", "behaviour": "An expired cart is emptied" }
    }
  },
  "architecture": {
    "entries": ["src/main.ts"],
    "forbidden": [{ "name": "ui-not-db", "from": ["src/ui/**"], "to": ["src/db/**"], "message": "Go through the service layer" }]
  },
  "deps": { "allow": [] },
  "rules": {}
}
```

- **`specs.items`** define promised behaviours; test titles start with the ID (`it('ORDER-01 sends a receipt', ...)`). T1 gaps block, T2 gaps warn.
- **`tiers`**, **`architecture`** and **`rules`** set risk paths, boundaries/entries/singletons/restricted strings, and mode overrides (`block` | `warn` | `off`). Lowering a mode or threshold counts as weakening (TEST-7).
- **`journeys`, `runbook`, `ci`, `testSuite`, `docs`, `duplication`, `mutation`, `scope`, `lock`, `holdout`, `review`, `provenance`, `evaluation`, `feedback`, `telemetry`** configure the remaining features (see `schema/guardrails.schema.json`).
- Add `.stryker-tmp/` to `.gitignore` when using mutation testing, and enable GitHub branch protection with **Require review from Code Owners** so `.github/CODEOWNERS` enforces human review. Set `GUARDRAILS_OFFLINE=1` to skip npm registry lookups.

Pull request approval labels come from `GUARDRAILS_LABELS` (comma-separated) or `--labels`:

| Label | Approves |
|---|---|
| `guardrails:journeys-approved` | Journey changes in the PR (JRN-3) |
| `guardrails:config-approved` | Loosening `guardrails.config.json` or `guardrails.md`, such as adding to `deps.allow` (TEST-7) |
| `guardrails:tests-removed-approved` | Deleting tests that existed on the base branch (TEST-7) |
| `guardrails:existing-behaviour-tests-approved` | New tests for existing behaviour that already pass on base (TEST-11) |
| `guardrails:baseline-approved` | Growing `.guardrails/baseline.json` or the test-suite baseline (BASE-1, SUITE-6) |
| `guardrails:flags-approved` | Merging while open agent flags remain in `.guardrails/flags.json` (FLAG-1) |

Legacy 0.6.1 aliases (`tests-removed-approved`, `guardrails:rebaseline`, `allow-characterization`, `flags-resolved`, and `guardrails:config-approved` for journeys) still work with a deprecation warning on stderr.

## Advanced verification (planned 0.7.0, all off by default)

A basic install behaves like `0.6.3`: every feature below is off by default until enabled in `guardrails.config.json` or `guardrails.md`.

| Feature | Switch | Command |
|---|---|---|
| Independent reviewer (REVIEW-1): sees only the merge-base diff and check output, never PR text, commit messages or reasoning. `stub` or `command` provider; runs on risky paths by default; sharded. | `review.enabled` | `guardrails review [--base ref] [--report f] [--shard i/n] [--format text\|json]`, `guardrails check --review` |
| Commit-pinned provenance (PROV-1): records tested SHA, parents, report sha256, version, checks and label overrides as JSON and a `<!-- guardrails:provenance v1 ... -->` marker; verifies head SHA and report hash. | `provenance.enabled` | `guardrails provenance record --report <f> [--head sha] [--events f] [--comments f] [--out f] [--format json\|markdown]`, `guardrails provenance verify --head <sha> [--file f] [--report f]` |
| Weekly checker evaluation: append/import `escaped-bug` and `false-escalation` ledger entries offline and propose rule soften/prune/sharpen/add actions (report only, never auto-applied). | `evaluation.enabled` | `guardrails ledger add <escaped-bug\|false-escalation> --pr <N> ...`, `guardrails ledger import --events <f>`, `guardrails evaluate [--since 7d\|ISO] [--cache dir] [--out f] [--format json]` |
| Feedback loop: tags blocking findings with `{rule, file, agent}` (`GUARDRAILS_AGENT_NAME` or `--agent-name`, default `unknown`) and clusters repeats into candidate rules for human review. | `feedback.enabled` | `guardrails feedback draft [--failures f] [--out f] [--format json]` |
| Label planner: scans the diff and prints every approval label needed (or already present) with `file:line`. Advisory only (always exits `0`, never adds labels). | always available | `guardrails labels plan [--base ref] [--labels a,b] [--format markdown\|json]` |
| Anonymous telemetry: opt-in POST on `init`, `check` and `report` with a short timeout; never alters exit codes. | `telemetry.enabled` + `telemetry.endpoint` | `guardrails telemetry status` |

Weekly workflow example: [`examples/guardrails-weekly.yml`](examples/guardrails-weekly.yml).

### `guardrails.md` and telemetry

`guardrails.md` sits at the repo root alongside `AGENTS.md`. Optional front matter takes flat `dotted.key: value` lines, and fenced ```` ```guardrails ```` or ```` ```json guardrails ```` blocks hold JSON objects. Precedence (lowest first): built-in defaults < fenced blocks (in order, objects deep-merged) < front matter < `guardrails.config.json`. Either file works alone; `journeys` and `deps` belong in `guardrails.config.json`. Agent hooks lock `guardrails.md`, CFG-2 requires a CODEOWNERS entry when it exists, and loosening it triggers TEST-7.

Telemetry is off unless `telemetry.enabled: true` and `telemetry.endpoint` are set; in CI (`CI` set), `GUARDRAILS_TELEMETRY=1` is also required. `GUARDRAILS_TELEMETRY=0` or `telemetry.enabled: false` disables it. There is no postinstall hook and no PII (only `v`, `event`, `version`, `node` major, `os`, `command`, `counts`, `durationMs`, and a cached random `installId`). Run `npx guardrails telemetry status` to inspect the exact status, reason and payload.

## CLI reference

Global options where applicable: `--cwd <dir>`, `--base <ref>`, `--labels <a,b>`, `--format text|json|markdown`, `--verbose`.

| Command | Purpose |
|---|---|
| `guardrails --help` | Print CLI commands and options (exit `0`; unknown commands exit `2`) |
| `guardrails init [--owner @handle] [--base main] [--preset site\|small-business] [--force]` | Scaffold or safely merge config, agent files, CODEOWNERS, workflow and hooks |
| `guardrails init runbook [--force]` | Write `RUNBOOK.md`, stub standard scripts, update `TESTING.md` and suite baseline, and enable `runbook`, `ci`, `testSuite` |
| `guardrails check [--base ref] [--fails-on-base] [--clock-shift] [--mutation] [--mutate globs] [--review] [--only groups] [--no-knip] [--specs IDs]` | Run checks (`--only`: `docs,runbook,ci,suite,setup,tests,lint,arch,gaming,flags,scope,journeys,failbase,clock,mutation,review`). Exit `1` on blocking findings |
| `guardrails report [--format text\|json\|markdown] [--report <file>]` | Run checks and print a report (exit `0`), or re-render a saved JSON `--report` without re-running |
| `guardrails baseline [--force]` | Create or ratchet `.guardrails/baseline.json` (only shrinks unless `--force`) |
| `guardrails specs [--format json]` | Print spec matrix: ID, tier, test count and behaviour |
| `guardrails explain [ID]` | List all 49 rules, or explain one (exit `2` if unknown) |
| `guardrails validate-test <ID> [--file f] [--lines n] [--base ref]` | Pre-flight check before writing a test (exit `1` on problems, `2` if `<ID>` missing) |
| `guardrails flag <ID> "<reason>" [--file f]` | Record a spec/test conflict (`reason` ≥ 10 chars) in `.guardrails/flags.json` (blocks merge via FLAG-1) |
| `guardrails mutants [--base ref] [--mutate globs]` | Recompute surviving Stryker mutants for changed files (JSON) |
| `guardrails hook-guard [--agent claude\|gemini\|antigravity\|codex]` | Pre-tool agent hook reading JSON on stdin; denies edits to locked files |
| `guardrails holdout --dir <checkout>` | Run hidden holdout tests from a private checkout (HOLD-1) |
| `guardrails journeys list [--format json]` | Journey coverage matrix (`journey -> specs -> tests`, e2e status) |
| `guardrails journeys draft [--dry-run]` | Draft up to `journeys.draft.max` (default 8, max 20) unapproved journeys (`--dry-run` uses heuristic only, no model call or write) |
| `guardrails journeys import <file.json>` | Atomically import unapproved journeys from a base-trusted producer |
| `guardrails journeys approve <ID> [--by name]` | Approve a journey so it gates merges (human-only) |
| `guardrails journeys scaffold` | Create missing `playwright.cuj.config.ts` and `e2e/cuj/.gitkeep` |
| `guardrails journeys generate-tests [--out e2e/cuj] [--check]` | Generate or verify (`--check`) standalone Playwright CUJ tests |
| `guardrails journeys run --base-url <url> [--ids A,B] [--only-enforced]` | Execute generated journeys (exit `1` on enforced failure, `2` on usage/setup error) |
| `guardrails affected [--base ref] [--e2e] [--run "<cmd>"]` | List or run test files affected by the diff |
| `guardrails check-tests [--update-baseline] [--force] [--format text\|json\|markdown]` | Audit test suite health (SUITE-1..6); exit `1` on regressions above baseline |
| `guardrails shard --shard i/n [--e2e] [--glob g] [--run "<cmd>"]` | Split files into size-balanced shards (`GUARDRAILS_SHARD` fallback) |
| `guardrails smoke --url <url> [--sitemap] [paths]` | Read-only GET checks (`SMOKE_URL` overrides `--url`); exit `1` on status ≥ 400 |
| `guardrails check-links <dir> [--require-contact]` | Verify internal HTML `href`/`src` targets in built output (exit `1` broken, `2` usage) |
| `guardrails secrets-check --require A,B -- <list cmd>` | Verify required secret names appear in provider output without printing values |

## Journeys (CUJs)

Journeys sit above specs: a human-approved P0 journey needs one Playwright test titled with its ID (JRN-1), orphan specs warn (JRN-2), journey edits require `guardrails:journeys-approved` unless imported by a base-trusted producer (JRN-3), and uncovered routes/forms warn (JRN-4). Declarative `run` steps (`goto`, `click`, `fill`, `submit`, `expectText`, `expectUrl`, `expectVisible`, `expectHref`, `expectNoHorizontalScroll`, `viewport`, `mockRoute`) compile via `journeys scaffold` + `journeys generate-tests` into `e2e/cuj/` (verified byte-for-byte by JRN-6) and run via `journeys run --base-url <url>` with `{{token}}` data from `.guardrails/client-data.json`, 10 s step timeouts, blocked service workers, and unmocked non-GET/HEAD requests aborted. `journeys draft` uses `GEMINI_API_KEY`, then `OPENAI_API_KEY`, else a local route heuristic; `--dry-run` never calls a model or writes files.

## Agents: hooks, skill and MCP

`init` configures `guardrails hook-guard` for Claude Code (`.claude/settings.json`), Gemini CLI (`.gemini/settings.json`, `.gemini/.env`), Antigravity (`.agents/hooks.json`) and Codex CLI (`.codex/hooks.json`, `.codex/config.toml` — trust the repo and `/hooks` in Codex). Hooks block edits and shell writes to `lock.paths` (`guardrails.config.json`, `guardrails.md`, `.guardrails/**`, `.github/CODEOWNERS`, `.github/workflows/**`, hook configs) and existing spec-tagged tests, while allowing `guardrails flag`. Humans unlock edits with `GUARDRAILS_ALLOW_TEST_EDITS=1`. Copy `skill/SKILL.md` into `.claude/skills/guardrails/SKILL.md`, `.gemini/skills/guardrails/SKILL.md` or `.agents/skills/guardrails/SKILL.md`.

`guardrails-mcp` (stdio, `cwd` or `GUARDRAILS_CWD`) exposes 9 tools: `guardrails_check`, `guardrails_report`, `guardrails_explain_rule`, `guardrails_list_specs`, `guardrails_validate_test`, `guardrails_flag`, `guardrails_mutants`, `guardrails_list_journeys` and `guardrails_journey_coverage` (read-only for journeys; no approve tool):

```bash
claude mcp add guardrails --scope project -- npx -y -p @ws42ca/guardrails guardrails-mcp
```

## CI and static sites

`init` writes `.github/workflows/guardrails.yml` (checkout with `fetch-depth: 0`, `GUARDRAILS_LABELS` from PR labels, `--base origin/$GITHUB_BASE_REF --fails-on-base` on PRs, `--clock-shift` on `main`/nightly, a separate `--only mutation --mutation` job, and gitleaks for SEC-1). `--fails-on-base` safely unlinks the temporary worktree `node_modules` junction/symlink before cleanup so your real `node_modules` is never touched. To run checks once and render multiple formats in CI, save JSON with `guardrails check --format json > report.json` and render `guardrails report --report report.json --format markdown`.

For static sites without adding a repo dependency, run `npx -y @ws42ca/guardrails init --preset site` to scaffold a site config and workflow using `check-links` and `smoke --sitemap`.

## Licence

MIT. Copyright (c) 2026 Josh Boyer / WS42. See `LICENSE`.
