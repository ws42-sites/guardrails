---
review.enabled: true
review.provider: stub
review.stub.failOn: ["^\\s*debugger;?\\s*$", "^\\s*//\\s*@ts-(ignore|nocheck)"]
provenance.enabled: true
evaluation.enabled: true
feedback.enabled: true
telemetry.enabled: false
---
# Guardrails settings for this repo

This repo dogfoods the 0.7.0 features. `guardrails.config.json` holds specs, tiers and rules and wins on any conflict. This file switches on the advanced checks:

- Independent reviewer: the deterministic `stub` provider, so CI makes no model calls. It runs only on PRs that touch locked tests, config or auth paths.
- Commit-pinned provenance: on every guardrails run in CI.
- The checker ledger and failure feedback, for the weekly evaluation.
- Telemetry stays off.
