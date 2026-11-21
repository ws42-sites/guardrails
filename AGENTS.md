# Agent instructions: @ws42ca/guardrails

This repo builds the guardrails framework. It follows its own rules (dogfood).

- Stack: Node ≥ 20, TypeScript (ESM, NodeNext), Vitest. Source is in `src/`, tests in `test/`, scaffolding in `template/`, the agent skill in `skill/`.
- Before committing, run `npm run build && npm test && npm run lint`. `lint` runs `guardrails check` on this repo, and it must pass.
- Testing rules: [TESTING.md](TESTING.md). Rule catalogue: [RULES.md](RULES.md). A new rule needs an entry in `src/rules.ts`, a row in RULES.md (a test checks they match), a spec in `guardrails.config.json` and one test.
- Every test title starts with a spec ID from `guardrails.config.json`. Keep test files under 300 lines.
- Don't add dependencies without a reason. ARCH-4 blocks them unless a code owner adds the package to `deps.allow`.
- If a test conflicts with the spec or seems impossible, STOP and call `guardrails_flag` (or `npx guardrails flag <SPEC-ID> "<reason>" --file <path>`). Never change code or tests just to satisfy a test.
- Rules regenerate into RULES.md with `node scripts/gen-rules.mjs` after `npm run build`.
- Never publish to npm, push, or create repos without the owner's go-ahead.
- CI fails when the README rule count or host list drifts, when a released version is still described as planned, when DOC-1 finds a file, or when the fresh-repo quickstart fails.
