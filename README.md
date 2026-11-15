# @ws42/guardrails

Config-driven checks that stop coding agents from padding tests, weakening them, sprawling docs or bypassing the architecture. One CLI, one MCP server, one reusable GitHub workflow, one config file per project.

- **Rules:** [RULES.md](RULES.md). **Testing policy:** [TESTING.md](TESTING.md). **Agent skill:** [skill/SKILL.md](skill/SKILL.md).
- **Engines:** dependency-cruiser (import boundaries), knip (production reachability), ESLint with eslint-plugin-vitest (test bans), a TypeScript AST inventory (spec IDs, weakening), jscpd (duplication on new code), the npm registry (hallucinated or risky packages), Stryker (mutation), a clock-shift preload (time bombs), a base-branch worktree (tests must fail without the change), and gitleaks in CI (secrets).
- **Anti-cheating:** product code that detects tests, exits early, overloads equality or hard-codes expected values blocks [ARCH-7]; agents stop and flag conflicts instead of bending tests [FLAG-1]; hooks lock spec-tagged tests for Claude Code, Gemini CLI and Antigravity; optional hidden holdout tests run only at the gate [HOLD-1].

## Install in a project
```bash
npm i -D -E @ws42/guardrails @stryker-mutator/core@8.7.1 @stryker-mutator/vitest-runner@8.7.1
npx guardrails init --owner @JoshBoyer --base main   # config, AGENTS.md + pointers, CODEOWNERS, CI caller, agent hooks + MCP
# edit guardrails.config.json: tiers, specs.items, architecture rules
npx guardrails baseline                               # record existing violations (only shrinks from here)
npx guardrails check                                  # add --base origin/main for diff rules, --clock-shift for TEST-6
```
Add `"@ws42/guardrails/vitest-setup"` to Vitest `setupFiles` so every test must assert (TEST-9), and add `.stryker-tmp/` to `.gitignore`. Turn on branch protection with "Require review from Code Owners" so CODEOWNERS actually guards the config, the baseline and CI.

## CLI
| Command | What it does |
|---|---|
| `guardrails init` | Scaffold `guardrails.config.json` (tiers guessed), `AGENTS.md`, one-line `GEMINI.md`/`CLAUDE.md`, `.github/CODEOWNERS`, and a workflow that calls the reusable one |
| `guardrails check [--base ref] [--clock-shift] [--mutation]` | Run the checks. Exit 1 on blocking findings |
| `guardrails report --format text\|json\|markdown` | Same checks, printed to stdout. Markdown is for PR comments and job summaries |
| `guardrails baseline [--force]` | Write or ratchet `.guardrails/baseline.json` |
| `guardrails specs`, `explain <ID>`, `validate-test <ID>` | Spec matrix, rule help, and a pre-flight check for a new test |
| `guardrails check --base ref --fails-on-base` | Also run TEST-11: new/changed tests must fail on base and pass on head |
| `guardrails flag <ID> "<reason>" [--file f]` | Stop-and-flag: record a spec/test conflict in `.guardrails/flags.json` (blocks merge, FLAG-1) |
| `guardrails mutants [--base ref]` | Recompute surviving mutants for changed files (JSON) |
| `guardrails hook-guard --agent claude\|gemini\|antigravity` | Agent hook: reads the tool call on stdin, denies edits to locked files |
| `guardrails holdout --dir <checkout>` | Run hidden holdout tests (HOLD-1) |
| `guardrails journeys list\|draft\|approve <ID>` | Journey coverage matrix; draft 5-10 unapproved journeys from docs, routes and mocks (Gemini if `GEMINI_API_KEY`, else OpenAI if `OPENAI_API_KEY`, else a route heuristic; `--dry-run`); approve (humans only) |
| `guardrails init --preset small-business` | Add starter journeys (services, contact form, book/call, trust, mobile, location/hours; menu/order/gallery when present) as drafts |
| `guardrails init runbook` | Write the standard `RUNBOOK.md` and stub `deploy`, `deploy:preview`, `rollback`, `smoke`, `secrets:check` scripts (never overwrites), and turn on RUN-1..4 |

PR labels come from `$GUARDRAILS_LABELS` (the workflow sets it): `tests-removed-approved` (delete base tests), `guardrails:rebaseline` (baseline may grow), `guardrails:config-approved` (config loosening), `allow-characterization` (new tests may pass on base), `flags-resolved` (open flags stop blocking). Only code owners should add them.

`GUARDRAILS_OFFLINE=1` skips the npm registry lookups (ARCH-4 then reports a metric instead). Results are cached for `deps.registry.cacheHours` in `node_modules/.cache/guardrails/`.

## Runbook standard (RUN-1..4)
Every repo ships one top-level `RUNBOOK.md` with the sections Deploy, Rollback, Smoke, Secrets rotation, Incident first steps (in that order), each pointing at an `npm run` script, never a click-path. The five script names are the same everywhere; one that genuinely does not apply goes in `runbook.na` with a reason. Deploy workflows call `npm run deploy` (no raw `wrangler`/`cdk`/... commands) and then `npm run smoke`. Other runbook/deploy docs are flagged unless listed in `runbook.allowDocs`. Off until `runbook.enabled` (set by `init runbook`).

DOC-1 (report-only) allows only top-level `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `README.md`, `ARCHITECTURE.md`, `TESTING.md`, `RUNBOOK.md` by default; markdown loaded at runtime (skills, prompts) goes in `docs.runtime` globs.

## Critical User Journeys
`journeys.items` in the config lists 5-10 plain-language user tasks (`CUJ-<AREA>`, priority P0-P2, steps, specs, routes). Only approved ones gate: approved P0 journeys need exactly one Playwright test under `e2e/` titled with the ID (JRN-1). The report shows a journey → specs → tests matrix with pass/fail when `journeys.resultsFile` points at Playwright's JSON reporter output. Model names: `journeys.draft.geminiModel` (default `gemini-2.5-flash`) and `openaiModel`. API keys are read from the environment only and never written anywhere. `journeys.visual` (JRN-5, per-step screenshot diff) is a stub for VIS-1 v2 and stays off.

## Agent hooks (test-file lock)
`init` writes (or merges into) `.claude/settings.json` (PreToolUse), `.gemini/settings.json` (BeforeTool) and `.agents/hooks.json` (Antigravity PreToolUse). Each calls `guardrails hook-guard`, which denies edits to `lock.paths` (config, `.guardrails/`, CODEOWNERS, workflows, the hook files) and to existing test files that contain a spec ID. New test files are fine. Shell commands that name a locked file together with a write (`sed -i`, `>`, `mv`, `rm`, `git checkout`…) are denied too; `guardrails flag` is always allowed. The hooks also deny `guardrails journeys approve|draft`, `init --preset` and attempts to unset `GUARDRAILS_AGENT` (Claude Code's settings set it via `env`, Gemini CLI via `.gemini/.env`). Shell routes can't be closed completely, so TEST-7 in CI is the backstop. A human unlocks with `GUARDRAILS_ALLOW_TEST_EDITS=1` in the agent's environment.

## MCP server (stdio)
Tools: `guardrails_list_journeys`, `guardrails_journey_coverage` (read-only; there is no approve tool), `guardrails_check`, `guardrails_report`, `guardrails_explain_rule`, `guardrails_list_specs`, `guardrails_validate_test`, `guardrails_flag` (stop and flag a conflict), `guardrails_mutants` (surviving mutants; the gate recomputes the score). The server checks the directory it is started in, or `GUARDRAILS_CWD`; every tool also takes `cwd`.

Until it's published, replace `npx -y -p @ws42/guardrails guardrails-mcp` with `node /path/to/guardrails/dist/mcp.js`.

**Gemini CLI** (`.gemini/settings.json` in the repo, or `~/.gemini/settings.json`):
```json
{ "mcpServers": { "guardrails": { "command": "npx", "args": ["-y", "-p", "@ws42/guardrails", "guardrails-mcp"], "cwd": "." } } }
```
**Antigravity** (app, IDE and `agy` CLI: `.agents/mcp_config.json` in the repo, or `~/.gemini/config/mcp_config.json`):
```json
{ "mcpServers": { "guardrails": { "command": "npx", "args": ["-y", "-p", "@ws42/guardrails", "guardrails-mcp"], "cwd": "." } } }
```
**Claude Code:** run `claude mcp add guardrails --scope project -- npx -y -p @ws42/guardrails guardrails-mcp`, or put the same `mcpServers` block in `.mcp.json`.

Skill: copy `skill/SKILL.md` to `.agents/skills/guardrails/SKILL.md` (Antigravity), `.claude/skills/guardrails/SKILL.md` (Claude Code) or `.gemini/skills/guardrails/SKILL.md` (Gemini CLI).

## CI
```yaml
jobs:
  guardrails:
    uses: ws42-sites/guardrails/.github/workflows/guardrails.yml@v0
    with: { clock-shift: true, mutation: true, fails-on-base: true, secret-scan: true, codeql: false }
```
- `check`: every rule in diff mode on PRs (including TEST-11), markdown report in the job summary, JSON uploaded.
- `mutation`: Stryker on changed files in the mutation tiers, with an incremental cache.
- `secrets` [SEC-1]: gitleaks (pinned `ghcr.io/gitleaks/gitleaks` image, redacted output) over the PR's commits. Blocks on any finding.
- `codeql` (optional, `codeql: true`): CodeQL `security-extended`. Needs GitHub Advanced Security on private repos.
- `holdout` (optional) [HOLD-1]: see below.

### Hidden holdout tests
1. Create a private repo the building agent can't read, e.g. `ws42-sites/<project>-holdout`, with Vitest test files (`*.test.ts`) that import the project by relative path from `holdout.target` (default `test/holdout/`).
2. Add `"holdout": { "repo": "ws42-sites/<project>-holdout", "specs": ["REVIEW-02"] }` to the config. Listed specs don't need visible tests (TEST-2), and adding specs to the list counts as loosening.
3. Create a fine-grained token with read access to that repo only, save it as the secret `GUARDRAILS_HOLDOUT_TOKEN`, and pass `holdout-repo` plus `secrets: { holdout-token: ${{ secrets.GUARDRAILS_HOLDOUT_TOKEN }} }` to the workflow.
4. Make sure `holdout.target` is inside your Vitest `include` and in `.gitignore`. The job copies the tests in, runs them, reports titles only, and deletes them.

## Develop
`npm run build && npm test && npm run lint`. `lint` dogfoods `guardrails check` on this repo. After changing `src/rules.ts`, run `node scripts/gen-rules.mjs` to regenerate the RULES.md table.
