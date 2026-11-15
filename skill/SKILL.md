---
name: guardrails
description: Use in any repo with a guardrails.config.json, before writing or changing tests, docs, dependencies or architecture, and before every commit. It explains how to pass the guardrails checks without padding or weakening tests.
---

# Working under @ws42/guardrails

The repo enforces its testing, docs and architecture rules with `guardrails`. CI blocks on the same checks, so run them yourself first.

## Before you write a test
1. Find the behaviour's spec ID: MCP `guardrails_list_specs`, or `npx guardrails specs`.
2. Validate it: MCP `guardrails_validate_test { specId, testFile, plannedLines }`, or `npx guardrails validate-test <ID> --file <path> --lines <n>`.
3. If the ID doesn't exist, stop. Ask the owner to add the behaviour to `specs.items`; don't invent an ID or edit the config yourself.
4. Start the title with the ID: `it("REVIEW-02 expired token returns 410", …)`. Prefer one `it.each` table over several near-identical tests.
5. Assert observable results: return values, written data, response status. Don't only assert that a mock was called.
6. Freeze time (`vi.useFakeTimers(); vi.setSystemTime(new Date("2026-01-01"))`). Use fixed inputs, no randomness.

## Before you commit
- Run MCP `guardrails_check { base: "origin/<base branch>" }`, or `npx guardrails check --base origin/<base>`. Fix every ✖. Read every ⚠.
- For a rule you don't understand, run `guardrails_explain_rule { id }` or `npx guardrails explain <ID>`.

## Never do these (they block, and a human has to approve them)
- Delete or loosen an existing test, remove an `expect`, or swap `toEqual` for `toMatchObject`/`toContain`, or `toBe` for `toBeTruthy`.
- Lower a threshold or mode, or widen an allowlist, in `guardrails.config.json`, Vitest or Stryker config.
- Add entries to `.guardrails/baseline.json` (it only shrinks; running `guardrails baseline` after a fix is fine).
- Add a dependency that isn't in `deps.allow`.
- Add markdown/txt docs outside `docs.allow`. Put the content in `AGENTS.md` or a check.
- Add `.only`, `.skip`, `test.todo`, snapshots in T1 tests, or `// Stryker disable`.

## If a check seems wrong
Say so in your report: name the rule ID, file and why. Don't work around it. Workarounds like moving test code into helpers, renaming files or splitting lines get caught by other rules and by review.
