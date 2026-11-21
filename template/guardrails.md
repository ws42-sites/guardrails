---
telemetry.enabled: false
---
# Guardrails

Version: 1 | 2026-11-20

Keep spec-tagged tests protected. Flag conflicts for a human to resolve.

Configuration precedence: defaults < fenced blocks < front matter < `guardrails.config.json`.
Objects merge; arrays and scalar values replace earlier values. Keep `journeys` and `deps` in the JSON file.

```guardrails
{ "review": { "enabled": false }, "provenance": { "enabled": false } }
```

Telemetry is opt-in and requires an endpoint. `GUARDRAILS_TELEMETRY=0` disables it;
continuous integration (CI) also requires `GUARDRAILS_TELEMETRY=1`.
`guardrails telemetry status` shows the reason and exact anonymous payload without sending it.
The payload contains only `v`, `event`, `version`, Node major (`node`), OS family (`os`),
`command`, `counts` (`block`, `warn`), `durationMs`, and a random `installId`.
