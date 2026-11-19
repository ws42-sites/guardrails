import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const script = new URL('../scripts/check-release.mjs', import.meta.url).href;
const { versionMatchesTag, changelogHas, packFiles, checkRelease } = await import(script);

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
    expect(checkRelease({ version: '0.5.0', tag: 'v0.4.0', changelog: '', files: [] })).toEqual([
      'Tag v0.4.0 does not match version v0.5.0', 'CHANGELOG.md missing ## [0.5.0]',
    ]);
  });
});

it.each([
  'package.json', 'README.md', 'LICENSE', 'CHANGELOG.md', 'RULES.md', 'TESTING.md',
  'dist/cli.js', 'journeys/example.json', 'schema/guardrails.schema.json',
  'skill/SKILL.md', 'template/AGENTS.md',
])('REL-02 allows packed file %s', path => {
  expect(packFiles([{ path }])).toEqual([]);
});

it.each([
  'src/cli.ts', 'reports/ops-report.md', '.env', '.env.local', 'test/release.test.ts',
  '.github/workflows/release.yml', '.guardrails/baseline.json', 'other.txt',
  'dist/.env.production', 'dist/../src/cli.ts', '/dist/cli.js', 'dist\\cli.js',
])('REL-02 rejects packed file %s', path => {
  expect(packFiles([{ path }])).toEqual([`Forbidden packed path: ${path}`]);
});

it('REL-02 checks the real package and dry-run tarball', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  const output = execFileSync(process.execPath,
    [process.env.npm_execpath!, 'pack', '--dry-run', '--json'], { encoding: 'utf8' });
  const files = JSON.parse(output).flatMap((pack: { files: { path: string }[] }) => pack.files);
  expect(files.map((file: { path: string }) => file.path)).toEqual(expect.arrayContaining([
    'package.json', 'LICENSE', 'CHANGELOG.md', 'dist/cli.js',
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
  expect(publishers[0]).toContain('[ -n "$NODE_AUTH_TOKEN" ] || { echo "NPM_TOKEN not set; skipping npm publish"; exit 0; }\n          npm publish --access public');
  expect(workflow).toContain('needs: verify');
});
