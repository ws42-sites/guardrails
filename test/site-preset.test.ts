import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { expect, it } from 'vitest';
import { parseConfig } from '../src/config.js';
import { init } from '../src/init.js';
import { siteTree } from './helpers/site.js';

it('SITE-01 validates the site template and writes only site files with the chosen base', () => {
  const template = JSON.parse(readFileSync('template/site/guardrails.config.json', 'utf8'));
  const cfg = parseConfig(template);
  expect([cfg.sourceRoots, cfg.tiers, cfg.journeys.items, cfg.runbook.enabled, cfg.testSuite.enabled, cfg.ci.enabled]).toEqual([
    ['src', 'functions'], { T1: ['functions/api/**'], T2: ['src/lib/**'], T3: ['src/**'] }, {}, false, false, false,
  ]);
  expect(cfg.journeys).toMatchObject({ e2eGlobs: ['e2e/cuj/**/*.spec.ts'], resultsFile: 'playwright-report/results.json', routeGlobs: ['src/pages/**', 'functions/**'] });
  const dir = siteTree();
  expect(init({ cwd: dir, preset: 'site', base: 'release' })).toEqual({ written: ['guardrails.config.json', '.github/workflows/guardrails.yml', '.gitignore'], skipped: [] });
  expect(JSON.parse(readFileSync(join(dir, 'guardrails.config.json'), 'utf8'))).toEqual({ ...template, baseBranch: 'release' });
  expect(readFileSync(join(dir, '.github/workflows/guardrails.yml'), 'utf8')).toContain('branches: [release]');
  expect(['AGENTS.md', 'GEMINI.md', 'CLAUDE.md', '.claude', '.gemini', '.agents', '.mcp.json'].filter((f) => existsSync(join(dir, f)))).toEqual([]);
});

it('SITE-01 keeps existing site files unless forced', () => {
  const dir = siteTree();
  init({ cwd: dir, preset: 'site' });
  writeFileSync(join(dir, 'guardrails.config.json'), '{"custom":true}');
  expect(init({ cwd: dir, preset: 'site' })).toEqual({ written: [], skipped: ['guardrails.config.json', '.github/workflows/guardrails.yml', '.gitignore'] });
  expect(readFileSync(join(dir, 'guardrails.config.json'), 'utf8')).toBe('{"custom":true}');
  expect(init({ cwd: dir, preset: 'site', force: true }).written).toEqual(['guardrails.config.json', '.github/workflows/guardrails.yml']);
  expect(parseConfig(JSON.parse(readFileSync(join(dir, 'guardrails.config.json'), 'utf8'))).baseBranch).toBe('main');
});

it('SITE-01 CLI selects site without changing the small-business journey preset', () => {
  const cli = (dir: string, preset: string) => spawnSync(process.execPath, [resolve('dist/cli.js'), 'init', '--cwd', dir, '--preset', preset], { encoding: 'utf8', env: { ...process.env, GUARDRAILS_AGENT: '' } });
  const site = siteTree();
  expect(cli(site, 'site').status).toBe(0);
  expect(existsSync(join(site, 'AGENTS.md'))).toBe(false);
  const small = siteTree();
  expect(cli(small, 'small-business').status).toBe(0);
  const cfg = parseConfig(JSON.parse(readFileSync(join(small, 'guardrails.config.json'), 'utf8')));
  expect(Object.keys(cfg.journeys.items).length).toBeGreaterThanOrEqual(5);
  expect(Object.values(cfg.journeys.items).every((j) => !j.approved)).toBe(true);
  expect(existsSync(join(small, '.claude/settings.json'))).toBe(true);
});

it('SITE-05 workflow pins standalone tooling once and builds and checks the site without CUJ steps', () => {
  const dir = siteTree();
  init({ cwd: dir, preset: 'site' });
  const workflow = readFileSync(join(dir, '.github/workflows/guardrails.yml'), 'utf8');
  const version = JSON.parse(readFileSync('package.json', 'utf8')).version as string;
  expect(workflow.match(new RegExp(version.replace(/\./g, '\\.'), 'g'))).toHaveLength(1);
  expect(workflow.match(/npx -y @ws42ca\/guardrails@\$\{GUARDRAILS_VERSION\}/g)).toHaveLength(3);
  expect(workflow.match(/npm ci/g)).toHaveLength(1);
  expect(workflow).not.toMatch(/npm (?:ci|install).*guardrails|playwright|cuj/i);
  for (const text of ['pull_request:', 'branches: [main]', 'workflow_dispatch:', 'node-version-file: .nvmrc', 'cache: npm', 'fetch-depth: 0', 'npm run build', 'check --only docs,lint,arch,gaming,journeys', 'check-links dist', 'astro preview --port 4321', 'smoke --url http://localhost:4321 --sitemap', 'GUARDRAILS_LABELS:', 'ghcr.io/gitleaks/gitleaks:v8.30.1']) expect(workflow).toContain(text);
});
