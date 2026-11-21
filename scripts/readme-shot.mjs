import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const OUT = 'docs/cli-weakened.svg';
const WIDTH = 92;

function git(dir, args) {
  execFileSync('git', ['-c', 'user.name=Shot', '-c', 'user.email=shot@example.com', '-c', 'commit.gpgsign=false', ...args], { cwd: dir, stdio: 'pipe' });
}

function wrap(line) {
  const lines = [];
  let rest = line;
  while (rest.length > WIDTH) {
    let at = rest.lastIndexOf(' ', WIDTH);
    if (at < 16) at = WIDTH;
    lines.push(rest.slice(0, at));
    rest = `  ${rest.slice(at).replace(/^ /, '')}`;
  }
  lines.push(rest);
  return lines;
}

function escapeXml(text) {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function svg(text) {
  const lines = text.replace(/\s+$/, '').split('\n').flatMap(wrap);
  const height = lines.length * 18 + 28;
  const body = lines.map((line, index) => {
    const color = line.startsWith('✖') || line.startsWith('FAIL') ? '#9f1239' : '#18181b';
    return `<text x="16" y="${26 + index * 18}" fill="${color}">${escapeXml(line)}</text>`;
  }).join('\n');
  return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="760" height="${height}" viewBox="0 0 760 ${height}" role="img">
<title>guardrails check blocks a loosened matcher</title>
<rect width="100%" height="100%" fill="#fafafa" stroke="#d4d4d8"/>
<style>text { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, Liberation Mono, monospace; font-size: 13px; }</style>
${body}
</svg>
`;
}

function capture() {
  const dir = mkdtempSync(join(tmpdir(), 'guardrails-shot-'));
  try {
    git(dir, ['init', '-q', '-b', 'main']);
    mkdirSync(join(dir, 'src'));
    const config = {
      version: 1,
      sourceRoots: ['src'],
      tiers: { T1: ['src/**'], T2: [], T3: [] },
      specs: { items: { 'CORE-01': { tier: 'T1', behaviour: 'adds two numbers' } } },
    };
    writeFileSync(join(dir, 'guardrails.config.json'), `${JSON.stringify(config, null, 2)}\n`);
    writeFileSync(join(dir, 'src/add.ts'), 'export const add = (a: number, b: number) => a + b;\n');
    writeFileSync(join(dir, 'src/add.test.ts'), "import { expect, it } from 'vitest';\nit('CORE-01 adds', () => { expect({ n: 1 + 1 }).toEqual({ n: 2 }); });\n");
    git(dir, ['add', '-A']);
    git(dir, ['commit', '-q', '-m', 'base']);
    writeFileSync(join(dir, 'src/add.test.ts'), "import { expect, it } from 'vitest';\nit('CORE-01 adds', () => { expect({ n: 1 + 1 }).toMatchObject({ n: 2 }); });\n");
    let out = '';
    try {
      out = execFileSync(process.execPath, [join(process.cwd(), 'dist/cli.js'), 'check', '--base', 'main', '--only', 'tests', '--cwd', dir], { encoding: 'utf8' });
    } catch (error) {
      out = error.stdout?.toString?.() ?? '';
      if (!out) throw error;
    }
    if (!out.includes('TEST-7') || !out.includes('toMatchObject') || !out.includes('FAIL:')) throw new Error(out);
    return svg(out);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const next = capture();
const check = process.argv.includes('--check');
if (check) {
  const current = readFileSync(OUT, 'utf8');
  if (current !== next) {
    console.error(`${OUT} does not match the current CLI output. Run node scripts/readme-shot.mjs`);
    process.exit(1);
  }
  console.log(`${OUT} matches`);
} else {
  writeFileSync(OUT, next);
  console.log(`wrote ${OUT}`);
}
