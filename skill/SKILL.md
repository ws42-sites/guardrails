---
name: guardrails
description: Use in any repo with a guardrails.config.json, before writing or changing tests, docs, dependencies or architecture, and before every commit. It explains how to pass the guardrails checks without padding or weakening tests.
---

# Working under @ws42/guardrails

The repo enforces its testing, docs and architecture rules with `guardrails`. CI blocks on the same checks, so run them yourself first.

## Journeys
The product is described as Critical User Journeys (MCP `guardrails_list_journeys`, `npx guardrails journeys list`). Check `guardrails_journey_coverage` to see which approved journeys lack an e2e test. Write the one e2e test for a journey under `e2e/`, titled with its ID (`test("CUJ-REVIEW client submits a choice", …)`), walking its steps. Never add, edit or approve journeys; if one seems missing or wrong, call `guardrails_flag`.

## Before you write a test
1. Find the behaviour's spec ID: MCP `guardrails_list_specs`, or `npx guardrails specs`.
2. Validate it: MCP `guardrails_validate_test { specId, testFile, plannedLines }`, or `npx guardrails validate-test <ID> --file <path> --lines <n>`.
3. If the ID doesn't exist, stop. Ask the owner to add the behaviour to `specs.items`; don't invent an ID or edit the config yourself.
4. Start the title with the ID: `it("REVIEW-02 expired token returns 410", …)`. Prefer one `it.each` table over several near-identical tests.
5. Assert observable results: return values, written data, response status. Don't only assert that a mock was called.
6. Freeze time (`vi.useFakeTimers(); vi.setSystemTime(new Date("2026-01-01"))`). Use fixed inputs, no randomness.

## Stop and flag
If a test conflicts with the spec, seems impossible, or would need a special case to pass: STOP and call MCP `guardrails_flag { specId, file, reason }` (or `npx guardrails flag <ID> "<reason>" --file <path>`). Then tell the user. Never change code or tests just to satisfy a test. Flagging is the right move, not a failure: open flags block merge until a human decides.

## Before you commit
- Run MCP `guardrails_check { base: "origin/<base branch>" }`, or `npx guardrails check --base origin/<base>`. Fix every ✖. Read every ⚠.
- In a PR branch add `--fails-on-base`: every new or changed test must fail on the base branch and pass on yours [TEST-11]. A test that already passes on base doesn't test your change.
- For TEST-8, MCP `guardrails_mutants` lists surviving mutants (file, line, mutator). Kill them with real assertions. Never report a mutation score yourself; CI recomputes it.
- For a rule you don't understand, run `guardrails_explain_rule { id }` or `npx guardrails explain <ID>`.

## Never do these (they block, and a human has to approve them)
- Delete or loosen an existing test, remove an `expect`, or swap `toEqual` for `toMatchObject`/`toContain`, or `toBe` for `toBeTruthy`.
- Lower a threshold or mode, or widen an allowlist, in `guardrails.config.json`, Vitest or Stryker config.
- Add entries to `.guardrails/baseline.json` (it only shrinks; running `guardrails baseline` after a fix is fine).
- Add a dependency that isn't in `deps.allow`.
- Add markdown/txt docs outside `docs.allow`. Put the content in `AGENTS.md` or a check.
- Add `.only`, `.skip`, `test.todo`, snapshots in T1 tests, or `// Stryker disable`.
- Make product code detect tests (`NODE_ENV === 'test'`, `VITEST`, `JEST_WORKER_ID`, `import.meta.vitest`), call `process.exit` outside a CLI entry, add `valueOf`/`equals`/`Symbol.toPrimitive` to pass comparisons, add `expect.extend`, count calls, or hard-code a test's expected value [ARCH-7].
- Copy-paste existing code instead of reusing it [ARCH-8]. Invent package names: new packages are checked against npm [ARCH-4].
- Edit spec-tagged tests or guardrails files through any route, including the shell. Hooks deny it; humans unlock with `GUARDRAILS_ALLOW_TEST_EDITS=1`.

## If a check seems wrong
Flag it (`guardrails_flag`) and say so in your report: name the rule ID, file and why. Don't work around it. Workarounds like moving test code into helpers, renaming files or splitting lines get caught by other rules and by review.
