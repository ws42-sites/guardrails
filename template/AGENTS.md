# Agent instructions

This is the one instruction file for every coding agent (GEMINI.md and CLAUDE.md just point here).

## Guardrails
- Run `npx guardrails check` before every commit; it must pass. In a PR branch run `npx guardrails check --base origin/{{BASE}}`.
- If your agent has the guardrails MCP server, call `guardrails_validate_test` before writing a test and `guardrails_check` before committing.
- Every test title starts with a spec ID from `guardrails.config.json` → `specs.items` (e.g. `it("REVIEW-03 expired token returns 410", …)`). New behaviour needs a new spec entry first; a code owner reviews it.
- Never delete or loosen an existing test, lower a threshold, or edit `guardrails.config.json`, `.guardrails/` or CI to get green. Those changes are blocked and need a code owner.
- If a test conflicts with the spec or seems impossible, STOP and call `guardrails_flag` (or `npx guardrails flag <SPEC-ID> "<reason>" --file <path>`). Never change code or tests just to satisfy a test. Open flags block merge until a human resolves them.
- Spec-tagged test files and guardrails files are locked by agent hooks (`.claude/settings.json`, `.gemini/settings.json`, `.agents/hooks.json`). Don't try to edit them through the shell either.
- Critical User Journeys in `guardrails.config.json` → `journeys` describe what users do. Approved P0 journeys need one e2e test titled with the journey ID. Agents never add or approve journeys (`guardrails journeys approve` is human-only).
- Testing rules: see the guardrails package `TESTING.md` (`node_modules/@ws42ca/guardrails/TESTING.md`).

## Project
<!-- Stack, commands, architecture notes. Keep this file under 150 lines; put rules in checks, not prose. -->
