# Testing

## Test suite standard (guardrails)
- **PRs** run `npm run test:changed` (affected tests only). **Nightly and before deploy**, `npm run test:full` runs everything [CI-1]. A slow full suite can shard: `guardrails shard --shard i/n --run "<runner>"` or `vitest run --shard=i/n`.
- **`npm run check:tests`** (`guardrails check-tests`) audits the suite. It flags `.only`, unexplained skips and source-text asserts [SUITE-1]; files over 300 lines [SUITE-2]; fixed sleeps [SUITE-3]; the real clock in assertions [SUITE-4]; and duplicate test families [SUITE-5]. It also reports the test/product line ratio.
- **Baseline:** existing findings are recorded in `.guardrails/test-suite-baseline.json`, and only regressions block. After a cleanup, run `npx guardrails check-tests --update-baseline` (it only ratchets down) and commit the file.
