# @ws42ca/guardrails

**Guardrails that stop AI coding agents from padding, weakening or bypassing your tests, docs and architecture.**

[![npm](https://img.shields.io/npm/v/@ws42ca/guardrails)](https://www.npmjs.com/package/@ws42ca/guardrails)
![license](https://img.shields.io/npm/l/@ws42ca/guardrails)

Coding agents optimise for a green build. Left alone, they delete the failing test, loosen an assertion, special-case the expected value, add a package that does not exist, or write a new doc for every task. `guardrails` is a config-driven checker that blocks those shortcuts in CI and inside the agent's own editor. It ships as one CLI, one MCP server, an agent skill and one config file per project.

Project page: [ws42.ca/guardrails](https://ws42.ca/guardrails)

## What it does

- **Keeps tests honest.** Every test maps to a spec ID, risky specs must have a test, and every test must assert something. Deleting tests, removing `expect`s, loosening matchers or lowering thresholds is blocked on pull requests. New tests must fail on the base branch (so they prove the change). Optional mutation testing with Stryker checks that tests catch real bugs.
- **Catches test-gaming.** Product code that detects the test runner, exits early, overloads equality or hard-codes expected values is blocked (ARCH-7).
- **Protects the architecture.** Import boundaries (dependency-cruiser), production reachability (knip), duplication on new code (jscpd), restricted strings and no fixtures in production code.
- **Checks new dependencies.** New packages must exist on the npm registry, be old enough and have enough downloads, and have no high or critical advisories. This catches hallucinated package names.
- **Describes the product as journeys.** Critical user journeys (CUJs) are plain-language tasks that humans approve. Approved journeys must have an end-to-end test, and the CLI can generate and run those tests.
- **Locks the rules.** Agent hooks for Claude Code, Gemini CLI, Antigravity and Codex CLI deny edits to the config, baseline, CI files and spec-tagged tests. Agents can stop and flag a conflict instead of bending a test, and the flag blocks merge until a human answers.
- **Keeps docs short.** One agent instruction file, an allowlist of top-level docs and a length cap.
- **Standardises runbooks and CI.** A required `RUNBOOK.md` that points at scripts, change-scoped test runs, and a test-suite audit with a baseline that only shrinks.

There are 47 rules. `npx guardrails explain` lists them, and `npx guardrails explain <ID>` says why a rule exists and how to fix a finding. The same catalogue ships in the package as `RULES.md`.

## Install

```bash
npm i -D @ws42ca/guardrails
```

Requires Node 20 or newer. Vitest, `@stryker-mutator/core` and `@stryker-mutator/vitest-runner` are optional peer dependencies. Install them to use the Vitest assertion setup and mutation testing:

```bash
npm i -D -E vitest @stryker-mutator/core@8.7.1 @stryker-mutator/vitest-runner@8.7.1
```

## Quick start

```bash
npx guardrails init --owner @your-github-handle --base main
```

`init` writes `guardrails.config.json`, `AGENTS.md` (the single source of truth for agent instructions), one-line `CLAUDE.md` and `GEMINI.md` pointers, `.github/CODEOWNERS`, a starter GitHub Actions workflow, and the agent hook and MCP files for Claude Code, Gemini CLI, Antigravity and Codex CLI. It never overwrites an existing file unless you pass `--force`. In a repository that already has these files, `init` merges instead: it appends the rules to `AGENTS.md` between `<!-- guardrails:start -->` and `<!-- guardrails:end -->` markers, adds a marked one-line pointer to an existing `CLAUDE.md` or `GEMINI.md` (unless it already mentions `AGENTS.md`), and appends missing owner lines to `.github/CODEOWNERS`. Running it again only replaces what is between the markers and never touches the rest. It prints `created`, `merged` or `skipped` for every file.

A fresh `init` followed by `npx guardrails check` passes. The runbook, CI and test-suite standards start off; opt in with `npx guardrails init runbook` once you want them (it writes `RUNBOOK.md` and stub scripts that fail until you wire them up).

Then:

```bash
# 1. Describe your behaviours and layers in guardrails.config.json (specs, tiers, architecture).
# 2. Record today's violations. The baseline can only shrink from here.
npx guardrails baseline

# 3. Run the checks. Exit code 1 means a blocking finding.
npx guardrails check

# On a branch, also run the diff rules (no test weakening, new tests fail on base, new dependencies):
npx guardrails check --base origin/main --fails-on-base
```

To make every test assert something (TEST-9), add the setup file to Vitest:

```ts
// vitest.config.ts
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: { setupFiles: ['@ws42ca/guardrails/vitest-setup'] },
});
```

Add `.stryker-tmp/` to `.gitignore` if you use mutation testing. Turn on branch protection with "Require review from Code Owners" so CODEOWNERS actually protects the config, the baseline and CI.

## Configuration

Everything lives in one `guardrails.config.json`. Unknown keys are an error (CFG-1), so a typo cannot silently turn a rule off. Add the schema for editor completion:

```json
{
  "$schema": "./node_modules/@ws42ca/guardrails/schema/guardrails.schema.json",
  "version": 1,
  "baseBranch": "main",
  "sourceRoots": ["src"],
  "tiers": {
    "T1": ["src/payments/**"],
    "T2": ["src/lib/**"],
    "T3": ["src/**"]
  },
  "specs": {
    "items": {
      "ORDER-01": { "tier": "T1", "behaviour": "A paid order sends a receipt email" },
      "ORDER-02": { "tier": "T2", "behaviour": "An expired cart is emptied" }
    }
  },
  "architecture": {
    "entries": ["src/main.ts"],
    "forbidden": [
      { "name": "ui-not-db", "from": ["src/ui/**"], "to": ["src/db/**"], "message": "Go through the service layer" }
    ]
  },
  "deps": { "allow": [] },
  "rules": {}
}
```

- **`specs.items`** are the promised behaviours. Test titles start with the ID, for example `it('ORDER-01 sends a receipt', ...)`. Tiers decide how strict the checks are: T1 specs without a test block, T2 gaps warn.
- **`tiers`** map paths to T1, T2 or T3. They also drive the test budget and which files are mutation-tested.
- **`architecture`** declares entry points, forbidden imports, singletons and restricted strings.
- **`rules`** overrides the default mode of a rule. Lowering a mode counts as weakening, so it needs approval on a pull request.
- **`journeys`, `runbook`, `ci`, `testSuite`, `docs`, `duplication`, `mutation`, `scope`, `lock`, `holdout`** configure the other checks. The schema file documents every option.

Pull request labels come from the `GUARDRAILS_LABELS` environment variable (comma separated) or `--labels`. Only code owners should be able to add them:

| Label | Effect |
|---|---|
| `guardrails:journeys-approved` | Approves the journey changes in the PR |
| `guardrails:config-approved` | Allows loosening the config |
| `guardrails:tests-removed-approved` | Allows deleting tests that exist on the base branch |
| `guardrails:existing-behaviour-tests-approved` | Allows new tests that lock in existing behaviour, so they already pass on base |
| `guardrails:baseline-approved` | Allows the baseline to grow |
| `guardrails:flags-approved` | Open flags stop blocking |

The old names (`tests-removed-approved`, `guardrails:rebaseline`, `allow-characterization`, `flags-resolved`, and `guardrails:config-approved` for journeys) still work in 0.6.1 as deprecated aliases and print a warning naming the new label.

Set `GUARDRAILS_OFFLINE=1` to skip the npm registry lookups.

## CLI

| Command | What it does |
|---|---|
| `guardrails init [--owner @handle] [--base main] [--force]` | Scaffold config, agent files, CODEOWNERS, CI workflow and hooks; merges into agent files and CODEOWNERS that already exist |
| `guardrails check [--base ref] [--fails-on-base] [--clock-shift] [--mutation] [--only groups]` | Run the checks. Exit 1 on blocking findings |
| `guardrails report --format text\|json\|markdown` | The same checks, printed. Markdown suits PR comments and job summaries |
| `guardrails baseline [--force]` | Write or ratchet `.guardrails/baseline.json` (it only shrinks) |
| `guardrails specs` | Print the spec matrix: ID, tier and test count |
| `guardrails explain [ID]` | List the rules, or explain one |
| `guardrails validate-test <ID> [--file f] [--lines n]` | Pre-flight check before writing a test |
| `guardrails flag <ID> "<reason>" [--file f]` | Stop and flag a spec/test conflict for a human (blocks merge, FLAG-1) |
| `guardrails mutants [--base ref]` | List surviving mutants for changed files (JSON) |
| `guardrails hook-guard --agent claude\|gemini\|antigravity\|codex` | Agent hook: reads a tool call on stdin, denies edits to locked files |
| `guardrails holdout --dir <checkout>` | Run hidden holdout tests (HOLD-1) |
| `guardrails journeys list` | Journey coverage matrix: journey, specs, tests |
| `guardrails journeys draft [--dry-run]` | Draft up to `journeys.draft.max` (default 8, at most 20) unapproved journeys from your docs, routes and mocks. `--dry-run` previews a route heuristic and never calls a model |
| `guardrails journeys approve <ID> [--by name]` | Approve a journey (humans only) |
| `guardrails journeys import <file.json>` | Import journeys from a trusted producer (never approves them) |
| `guardrails journeys generate-tests [--out dir] [--check]` | Generate or verify Playwright tests from journeys |
| `guardrails journeys scaffold` | Create the missing Playwright journey config |
| `guardrails journeys run --base-url <url> [--ids A,B] [--only-enforced]` | Run the journeys against a running site |
| `guardrails init --preset small-business` | Add starter journeys as unapproved drafts |
| `guardrails init runbook` | Write `RUNBOOK.md` and stub `deploy`, `deploy:preview`, `rollback`, `smoke` and `secrets:check` scripts |
| `guardrails affected [--base ref] [--e2e] [--run "<cmd>"]` | Test files affected by the change (dependency graph) |
| `guardrails check-tests [--update-baseline]` | Audit the test suite (SUITE-1 to 6) |
| `guardrails shard --shard i/n [--e2e] [--run "<cmd>"]` | Size-balanced test shards for full-suite workflows |
| `guardrails smoke --url <url> [--sitemap] [paths]` | Read-only GET of each path, plus every page in the sitemap with `--sitemap`. Fails on a status of 400 or more |
| `guardrails check-links <dir> [--require-contact]` | Check internal `href` and `src` references in built HTML. Exit 1 on broken links |
| `guardrails init --preset site` | Scaffold a site config and GitHub Actions workflow for a built static site, without agent files |
| `guardrails secrets-check --require A,B -- <list cmd>` | Fails unless every named secret appears in the provider's list output (names only, never printed) |

## Journeys (CUJs)

A critical user journey is a short, plain-language task a real visitor does: "Visitor sends an enquiry through the contact form and sees a confirmation". Journeys give the checks something other than the code to hold agents to. Unit tests can all pass while the form is broken.

```json
{
  "journeys": {
    "items": {
      "CUJ-CHECKOUT": {
        "title": "Visitor buys a product and sees a confirmation",
        "priority": "P0",
        "steps": ["Open the shop", "Add an item", "Pay", "See the confirmation"],
        "specs": ["ORDER-01"],
        "routes": ["/shop"],
        "run": [
          { "goto": "/shop" },
          { "click": { "role": "button", "name": "Add to cart" } },
          { "expectText": "Order confirmed" }
        ]
      }
    }
  }
}
```

- **Humans own them.** Agents can draft journeys, but only a person runs `guardrails journeys approve CUJ-CHECKOUT`. Approved and client-provided journeys are protected from edits (JRN-3).
- **Approved journeys gate.** An approved P0 journey needs exactly one end-to-end test titled with its ID (JRN-1). Specs that serve no approved journey warn (JRN-2), and new routes and forms that no journey covers are flagged (JRN-4).
- **Executable.** A journey with a declarative `run` list can be turned into Playwright tests and run against a local preview, a staging site or production:

  ```bash
  npx guardrails journeys scaffold
  npx guardrails journeys generate-tests
  npx guardrails journeys run --base-url http://localhost:4321
  ```

  The step vocabulary is `goto`, `click`, `fill`, `submit`, `expectText`, `expectUrl`, `expectVisible`, `expectHref`, `expectNoHorizontalScroll`, `viewport` and `mockRoute`. There is no free-form code in a journey. The site only needs `@playwright/test`, and generated tests never import guardrails. Hand edits to generated files and stale files are detected (JRN-6).
- **Enforced versus advisory.** Failures of approved or client-provided journeys exit 1. Unapproved inferred or template journeys only warn.
- **Safe by default.** Every step has a 10-second timeout. Requests other than GET and HEAD that are not mocked are aborted, and service workers are blocked so they cannot bypass routing. Values written as `{{token}}` resolve from `.guardrails/client-data.json`.
- **Drafting.** `journeys draft` uses Gemini if `GEMINI_API_KEY` is set, otherwise OpenAI if `OPENAI_API_KEY` is set, otherwise a route-based heuristic. `--dry-run` always uses the heuristic, so it makes no network or model call even when a key is set. Keys are read from the environment and never written anywhere.

## Agents: hooks, skill and MCP

`init` writes hook settings for Claude Code (`.claude/settings.json`), Gemini CLI (`.gemini/settings.json`), Antigravity (`.agents/hooks.json`) and Codex CLI (`.codex/hooks.json`). Each calls `guardrails hook-guard`, which denies edits to the config, `.guardrails/`, CODEOWNERS, workflows, the hook files and existing test files that carry a spec ID. New test files are fine. Shell commands that name a locked file together with a write are denied too, and `guardrails flag` is always allowed. Shell routes cannot be closed completely, so the CI checks are the backstop. A human unlocks edits with `GUARDRAILS_ALLOW_TEST_EDITS=1`.

The MCP server (`guardrails-mcp`, stdio) exposes `guardrails_list_journeys`, `guardrails_journey_coverage`, `guardrails_check`, `guardrails_report`, `guardrails_explain_rule`, `guardrails_list_specs`, `guardrails_validate_test`, `guardrails_flag` and `guardrails_mutants`. It is read-only for journeys, and there is no approve tool. It checks the directory it starts in, or `GUARDRAILS_CWD`.

```bash
# Claude Code
claude mcp add guardrails --scope project -- npx -y -p @ws42ca/guardrails guardrails-mcp
```

```json
{ "mcpServers": { "guardrails": { "command": "npx", "args": ["-y", "-p", "@ws42ca/guardrails", "guardrails-mcp"], "cwd": "." } } }
```

The second block works for Gemini CLI (`.gemini/settings.json`) and Antigravity (`.agents/mcp_config.json`). The agent skill that tells an agent how to work under these rules is in the package at `skill/SKILL.md`. Copy it to `.claude/skills/guardrails/SKILL.md`, `.gemini/skills/guardrails/SKILL.md` or `.agents/skills/guardrails/SKILL.md`.

### Codex CLI

`init` writes `.codex/config.toml` (the MCP server, and `GUARDRAILS_AGENT=codex` for Codex's shell so human-only commands refuse) and `.codex/hooks.json` (a `PreToolUse` hook for shell commands and `apply_patch`). Codex reads `AGENTS.md` itself, so it needs no pointer file. An existing `config.toml` is only appended to, never rewritten.

- Codex loads project `.codex/` files only for trusted projects, and skips hooks you have not reviewed. Trust the project, then open `/hooks` and trust the guardrails hook.
- The hook denies a patch or shell write to a locked file with exit code 2, as Claude Code's does. Codex documents hooks as a guardrail, not a complete boundary, so the CI checks are still the backstop.
- Antigravity has no hook-config way to set an environment variable, so `GUARDRAILS_AGENT` is not set for it. Its hook still denies the same edits; the human-only journey commands are only blocked in Antigravity by that hook.

## CI

Run the same command CI-side with the base branch, so the diff rules apply. This GitHub Actions job also writes the report to the job summary:

```yaml
name: Guardrails
on:
  pull_request:
    types: [opened, synchronize, reopened, labeled, unlabeled]
  push:
    branches: [main]

permissions:
  contents: read
  pull-requests: read

jobs:
  guardrails:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: npm }
      - run: npm ci
      - name: guardrails
        env:
          GUARDRAILS_LABELS: ${{ join(github.event.pull_request.labels.*.name, ',') }}
        run: |
          ARGS=""
          if [ "${{ github.event_name }}" = "pull_request" ]; then
            ARGS="--base origin/${{ github.base_ref }} --fails-on-base"
          fi
          npx guardrails report --format markdown $ARGS >> "$GITHUB_STEP_SUMMARY" || true
          npx guardrails check $ARGS
```

Notes:

- `guardrails check` exits 1 on blocking findings. The checks need full git history (`fetch-depth: 0`) to compare against the base branch. `--fails-on-base` unlinks the worktree `node_modules` link first during cleanup so the real `node_modules` directory is never deleted.
- Mutation testing is opt in: add `npx guardrails check --only mutation --mutation --base origin/<base>` as a separate job. It runs Stryker on changed files in the mutation tiers.
- The clock-shift rerun (`--clock-shift`, TEST-6) runs the whole suite with the clock moved forward. Run it on the main branch and nightly, not on every pull request.
- Secret scanning (SEC-1) is a gitleaks step in CI, not part of the CLI.
- Optional hidden holdout tests (HOLD-1) run with `guardrails holdout --dir <checkout>` against tests in a private repository that the building agent cannot read.
- Change-scoped CI: `guardrails affected --base origin/main --run "npx vitest run"` runs only the tests the change can reach, with the full suite on a nightly schedule.

## Static sites

For a built static site, `npx -y @ws42ca/guardrails init --preset site` writes a site-sized config and a GitHub Actions workflow, and keeps guardrails out of the site's own dependencies. The workflow runs a pinned version through `npx`: set `GUARDRAILS_VERSION` in it to the release you want.

`guardrails check-links dist` checks internal `href` and `src` references against the built files, including directory indexes and extensionless pages. Broken links print `file:line -> target`. Add `--require-contact` to require a `tel:` or `mailto:` link on `index.html`.

`guardrails smoke --url http://localhost:4321 --sitemap` fetches the pages listed in `/sitemap-index.xml` or `/sitemap.xml` (following nested indexes) from a preview server, and fails if the sitemap is missing or empty.

## Also included

- **Runbook standard (RUN-1 to 4).** One top-level `RUNBOOK.md` with Deploy, Rollback, Smoke, Secrets rotation and Incident first steps, each pointing at an `npm run` script. Deploy workflows call the script and then `npm run smoke`.
- **Test-suite health (SUITE-1 to 6).** `guardrails check-tests` flags `.only`, unexplained skips, tests over 300 lines, fixed sleeps, the real clock in assertions and duplicate test families. A committed baseline means only regressions block.
- **Change-scoped CI (CI-1).** Warns when a PR workflow runs the full suite unguarded or no scheduled workflow runs it.

## Licence

MIT. Copyright (c) 2026 Josh Boyer / WS42. The full text is in the `LICENSE` file in the package.
