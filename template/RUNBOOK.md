# Runbook

Every step is a command. No click-paths: if a step needs a dashboard, script it first.
Standard scripts (same names in every repo): `deploy`, `deploy:preview`, `rollback`, `smoke`, `secrets:check`.
A step that does not apply is listed in `guardrails.config.json` → `runbook.na` with the reason.

## Deploy

How it ships: TODO (name the real mechanism, e.g. Cloudflare Pages via wrangler).

```sh
npm run deploy:preview   # preview environment
npm run deploy           # production (CI runs this, then smoke)
```

## Rollback

```sh
npm run rollback
```

## Smoke

Read-only checks against the deployed URL. Set `SMOKE_URL` to test a preview. For static sites, use `guardrails smoke --url <url> --sitemap` to include sitemap pages, and `guardrails check-links dist` to check built internal links before deployment (`--require-contact` also requires a home-page phone or email link).

```sh
npm run smoke
```

## Secrets rotation

Check the required secrets exist (names only, never values), rotate at the provider, then re-check.

```sh
npm run secrets:check
```

## Incident first steps

1. Confirm: `npm run smoke`
2. If the last deploy caused it: `npm run rollback`, then `npm run smoke`
3. Record what happened in the PR or issue that fixes it.
