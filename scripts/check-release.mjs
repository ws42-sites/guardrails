import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export function versionMatchesTag(pkgVersion, ref) {
  return !ref || (ref.startsWith('v') && ref.slice(1) === pkgVersion);
}

export function changelogHas(version, text) {
  return text.split(/\r?\n/).some(line => line.startsWith(`## [${version}]`));
}

export function packFiles(list) {
  const files = new Set(['package.json', 'README.md', 'LICENSE', 'CHANGELOG.md', 'RULES.md', 'TESTING.md']);
  const roots = new Set(['dist', 'journeys', 'schema', 'skill', 'template']);
  const forbidden = new Set(['reports', 'test', 'src', '.github', '.guardrails']);
  const required = 'template/.gemini/.env';
  const problems = list.flatMap(file => {
    const path = typeof file === 'string' ? file : file.path;
    if (typeof path !== 'string') return ['Invalid packed path'];
    const parts = path.split('/');
    const unsafe = forbidden.has(parts[0]) || path.includes('\\') || parts.some(part =>
      !part || part === '.' || part === '..' || (part.startsWith('.env') && path !== required));
    return !unsafe && (files.has(path) || (parts.length > 1 && roots.has(parts[0])))
      ? [] : [`Forbidden packed path: ${path}`];
  });
  if (!list.some(file => (typeof file === 'string' ? file : file.path) === required)) {
    problems.push(`missing required template file: ${required}`);
  }
  return problems;
}

export function checkRelease({ version, tag, changelog, files }) {
  const problems = [];
  if (!versionMatchesTag(version, tag)) problems.push(`Tag ${tag} does not match version v${version}`);
  if (!changelogHas(version, changelog)) problems.push(`CHANGELOG.md missing ## [${version}]`);
  return [...problems, ...packFiles(files)];
}

function main() {
  try {
    const args = process.argv.slice(2);
    if (args.length && (args.length !== 2 || args[0] !== '--tag' || !args[1])) {
      throw new Error('Usage: node scripts/check-release.mjs [--tag vX.Y.Z]');
    }
    const tag = args[1] ?? process.env.GITHUB_REF_NAME;
    if (!tag) console.log('No tag supplied; version/tag check skipped');
    const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
    // npm_execpath avoids shell invocation on Windows as well as lifecycle recursion.
    const output = process.env.npm_execpath
      ? execFileSync(process.execPath, [process.env.npm_execpath, 'pack', '--dry-run', '--json'], { encoding: 'utf8' })
      : execFileSync(process.platform === 'win32' ? 'npm.cmd' : 'npm', ['pack', '--dry-run', '--json'], { encoding: 'utf8', shell: process.platform === 'win32' });
    const problems = checkRelease({ version: pkg.version, tag,
      changelog: readFileSync('CHANGELOG.md', 'utf8'), files: JSON.parse(output).flatMap(pack => pack.files) });
    for (const problem of problems) console.error(problem);
    process.exitCode = problems.length ? 1 : 0;
  } catch (error) {
    console.error(String(error.message).replace(/[\r\n]+/g, ' '));
    process.exitCode = 1;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
