# Runbook

Every step is a command. Standard scripts: `deploy`, `deploy:preview`, `rollback`, `smoke`, `secrets:check`.
Steps that do not apply are listed with their reason in `guardrails.config.json` → `runbook.na`.

## Deploy

n/a as an application deploy. This library is published from CI.

1. `package.json` `version` is the release version, and `CHANGELOG.md` has `## [that version]` with the notes under it. Move the `[Unreleased]` notes into that heading.
2. `npm run build && npm test && npm run lint && npm run check:docs && npm run check:audit`
3. Commit on `main`, then tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`
4. `.github/workflows/release.yml` runs on the tag. It verifies the build, tests, docs checks, pack allowlist and changelog, then creates a GitHub release whose notes are that version's changelog section and runs `npm publish --access public --provenance`.

The publish step needs the repository secret `NPM_TOKEN` (an npm Automation token) and a public repository. Provenance uses `id-token: write`, which the workflow already requests. This repo cannot create that secret from a pull request. Without `NPM_TOKEN`, or while the repository is private, the tag workflow fails before it publishes.

## Rollback

n/a as an application rollback. Publish a newer patch that restores the previous behaviour. `npm unpublish` is only available for 72 hours, and only for versions that nothing depends on.

## Smoke

Checks the built CLI loads and passes its own doc and runbook rules.

```sh
npm run build && npm run smoke
```

The fresh-repo quickstart (pack, install, init, baseline, check) is `npm run check:quickstart`.

## Secrets rotation

n/a: no runtime secrets. CI secret scanning runs gitleaks (SEC-1). Rotate `NPM_TOKEN` from the npm website, then replace the `NPM_TOKEN` repository secret.

## Incident first steps

1. Reproduce: `npm run smoke`, then `npm test`.
2. If a release broke consumers, publish a fixed patch (see Rollback) and mark the bad version deprecated with `npm deprecate @ws42ca/guardrails@X.Y.Z "reason"`.
