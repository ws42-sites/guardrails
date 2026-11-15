# @ws42/guardrails

Config-driven checks that stop coding agents from padding tests, weakening them, sprawling docs or bypassing the architecture. One CLI, one MCP server, one reusable GitHub workflow, one config file per project.

- **Rules:** [RULES.md](RULES.md). **Testing policy:** [TESTING.md](TESTING.md). **Agent skill:** [skill/SKILL.md](skill/SKILL.md).
- **Engines:** dependency-cruiser (import boundaries), knip (production reachability), ESLint with eslint-plugin-vitest (test bans), a TypeScript AST inventory (spec IDs, weakening), Stryker (mutation), and a clock-shift preload (time bombs).

## Install in a project
```bash
npm i -D -E @ws42/guardrails @stryker-mutator/core@8.7.1 @stryker-mutator/vitest-runner@8.7.1
npx guardrails init --owner @JoshBoyer --base main   # config, AGENTS.md + pointers, CODEOWNERS, CI caller
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

PR labels come from `$GUARDRAILS_LABELS` (the workflow sets it): `tests-removed-approved` allows deleting base tests, and `guardrails:rebaseline` allows the baseline to grow.

## MCP server (stdio)
Tools: `guardrails_check`, `guardrails_report`, `guardrails_explain_rule`, `guardrails_list_specs`, `guardrails_validate_test`. The server checks the directory it is started in, or `GUARDRAILS_CWD`; every tool also takes `cwd`.

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
    with: { clock-shift: true, mutation: true }
```
The `check` job runs every rule in diff mode on PRs, writes the markdown report to the job summary, and uploads the JSON. The `mutation` job runs Stryker on changed files in the mutation tiers, with an incremental cache.

## Develop
`npm run build && npm test && npm run lint`. `lint` dogfoods `guardrails check` on this repo.
