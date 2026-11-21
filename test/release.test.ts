import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { initTemplateFiles } from '../src/init.js';

const script = new URL('../scripts/check-release.mjs', import.meta.url).href;
const { versionMatchesTag, changelogHas, packFiles, checkRelease } = await import(script);
const requiredTemplate = 'template/.gemini/.env';

function realPackFiles(): { path: string }[] {
  const args = ['pack', '--dry-run', '--json'];
  const output = process.env.npm_execpath
    ? execFileSync(process.execPath, [process.env.npm_execpath, ...args], { encoding: 'utf8' })
    : execFileSync(process.platform === 'win32' ? 'npm.cmd' : 'npm', args,
      { encoding: 'utf8', shell: process.platform === 'win32' });
  return JSON.parse(output).flatMap((pack: { files: { path: string }[] }) => pack.files);
}

describe('REL-01 release identity', () => {
  it.each([
    ['v0.5.0', true], ['v0.4.0', false], ['0.5.0', false],
    [undefined, true], ['', true], ['vv0.5.0', false],
  ])('REL-01 validates tag %s', (tag, valid) => {
    expect(versionMatchesTag('0.5.0', tag)).toBe(valid);
  });

  it.each([
    ['## [0.5.0] - 2026-11-15', true],
    ['# Changelog\r\n## [0.5.0]\r\n', true],
    ['## [0.4.0]', false], ['text ## [0.5.0]', false],
    ['## [0x5x0]', false], ['## [0.5.00]', false],
  ])('REL-01 requires an exact version heading in %s', (text, valid) => {
    expect(changelogHas('0.5.0', text)).toBe(valid);
  });

  it('REL-01 reports all identity problems together', () => {
    expect(checkRelease({ version: '0.5.0', tag: 'v0.4.0', changelog: '', files: [requiredTemplate] })).toEqual([
      'Tag v0.4.0 does not match version v0.5.0', 'CHANGELOG.md missing ## [0.5.0]',
    ]);
  });
});

it.each([
  'package.json', 'README.md', 'LICENSE', 'CHANGELOG.md', 'RULES.md', 'TESTING.md',
  'dist/cli.js', 'journeys/example.json', 'schema/guardrails.schema.json',
  'skill/SKILL.md', 'template/AGENTS.md', requiredTemplate,
])('REL-02 allows packed file %s', path => {
  expect(packFiles([{ path }, { path: requiredTemplate }])).toEqual([]);
});

it.each([
  'src/cli.ts', 'reports/ops-report.md', '.env', '.env.local', 'test/release.test.ts',
  '.github/workflows/release.yml', '.guardrails/baseline.json', 'other.txt',
  'dist/.env.production', 'dist/../src/cli.ts', '/dist/cli.js', 'dist\\cli.js',
  'src/.env.local', 'template/.env', 'template/.gemini/.env.local',
])('REL-02 rejects packed file %s', path => {
  expect(packFiles([{ path }, { path: requiredTemplate }])).toEqual([`Forbidden packed path: ${path}`]);
});

it.each([{ files: [] }, { files: ['package.json', 'template/AGENTS.md'] },
  { files: [{ path: 'template/.gemini/settings.json' }] }])(
  'REL-02 rejects a pack list missing the required Gemini template $files', ({ files }) => {
    expect(packFiles(files)).toEqual([`missing required template file: ${requiredTemplate}`]);
  });

it('REL-02 packs every template required by init', () => {
  const paths = realPackFiles().map(file => file.path);
  expect(paths).toEqual(expect.arrayContaining(initTemplateFiles().map(file => `template/${file}`)));
});

it('REL-02 checks the real package and dry-run tarball', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  const files = realPackFiles();
  expect(files.map((file: { path: string }) => file.path)).toEqual(expect.arrayContaining([
    'package.json', 'LICENSE', 'CHANGELOG.md', 'dist/cli.js', requiredTemplate, 'docs/cli-weakened.svg',
  ]));
  expect(checkRelease({ version: pkg.version, tag: `v${pkg.version}`,
    changelog: readFileSync('CHANGELOG.md', 'utf8'), files })).toEqual([]);
});

it('REL-03 publishes only in the token-guarded tag workflow step', () => {
  const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
  expect(workflow.match(/^on:\r?\n([\s\S]*?)\r?\npermissions:/m)?.[1].trim())
    .toBe("push:\n    tags: ['v*']");
  const steps = workflow.split(/^      - /m);
  const publishers = steps.filter(step => /^\s*npm publish\b/m.test(step));
  expect(workflow.match(/^\s*npm publish\b/gm)).toHaveLength(1);
  expect(publishers).toHaveLength(1);
  expect(publishers[0]).toContain('NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}');
  expect(publishers[0]).toContain('NPM_TOKEN is not set');
  expect(publishers[0]).toContain('exit 1');
  expect(publishers[0]).toContain('npm publish --access public --provenance');
  expect(publishers[0]).not.toContain('skipping npm publish');
  expect(workflow).toContain('needs: verify');
  expect(workflow).toContain('--notes-file release-notes.md');
});
