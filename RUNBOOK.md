# Runbook

Every step is a command. Standard scripts: `deploy`, `deploy:preview`, `rollback`, `smoke`, `secrets:check`.
Steps that do not apply are listed with their reason in `guardrails.config.json` → `runbook.na`.

## Deploy

n/a: this is a library. Consumers vendor a tarball built with `npm pack` (after `npm run build`) into `tools/guardrails` or depend on it from git.

## Rollback

n/a: each consumer pins its vendored tarball; roll back by re-vendoring the previous tarball in that repo.

## Smoke

Checks the built CLI loads and passes its own doc and runbook rules.

```sh
npm run build && npm run smoke
```

## Secrets rotation

n/a: no runtime secrets. CI secret scanning runs gitleaks (SEC-1).

## Incident first steps

1. Reproduce: `npm run smoke`, then `npm test`.
2. If a release broke consumers, they re-vendor the previous tarball (see Rollback).
