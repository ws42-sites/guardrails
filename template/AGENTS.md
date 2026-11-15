# Agent instructions

This is the one instruction file for every coding agent (GEMINI.md and CLAUDE.md just point here).

## Guardrails
- Run `npx guardrails check` before every commit; it must pass. In a PR branch run `npx guardrails check --base origin/{{BASE}}`.
- If your agent has the guardrails MCP server, call `guardrails_validate_test` before writing a test and `guardrails_check` before committing.
- Every test title starts with a spec ID from `guardrails.config.json` → `specs.items` (e.g. `it("REVIEW-03 expired token returns 410", …)`). New behaviour needs a new spec entry first; a code owner reviews it.
- Never delete or loosen an existing test, lower a threshold, or edit `guardrails.config.json`, `.guardrails/` or CI to get green. Those changes are blocked and need a code owner.
- Testing rules: see the guardrails package `TESTING.md` (`node_modules/@ws42/guardrails/TESTING.md`).

## Project
<!-- Stack, commands, architecture notes. Keep this file under 150 lines; put rules in checks, not prose. -->
