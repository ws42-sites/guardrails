import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const root = process.cwd();
const packed = JSON.parse(execFileSync('npm', ['pack', '--json'], { cwd: root, encoding: 'utf8' }));
const filename = packed[0].filename;
const tarball = join(root, filename);
const dir = mkdtempSync(join(tmpdir(), 'guardrails-quickstart-'));
const env = { ...process.env, GUARDRAILS_OFFLINE: '1', GUARDRAILS_TELEMETRY: '0' };
for (const key of Object.keys(env)) {
  if (key === 'CI' || key.startsWith('GITHUB_') || key.startsWith('GUARDRAILS_')) delete env[key];
}
env.GUARDRAILS_OFFLINE = '1';
env.GUARDRAILS_TELEMETRY = '0';
const run = (cmd, args, cwd) => execFileSync(cmd, args, { cwd, encoding: 'utf8', env });

try {
  execFileSync('git', ['init', '-q', '-b', 'main'], { cwd: dir, env });
  writeFileSync(join(dir, 'package.json'), `${JSON.stringify({ name: 'fresh-app', version: '1.0.0', private: true }, null, 2)}\n`);
  mkdirSync(join(dir, 'src'));
  writeFileSync(join(dir, 'src/main.ts'), 'export const appName = "fresh-app";\n');
  execFileSync('npm', ['install', '--no-fund', '--no-audit', '--ignore-scripts', tarball], { cwd: dir, env, stdio: 'inherit' });
  const cli = join(dir, 'node_modules', '.bin', 'guardrails');
  run(cli, ['init', '--owner', '@josh', '--base', 'main'], dir);
  run(cli, ['baseline'], dir);
  const check = run(cli, ['check'], dir);
  const ignore = readFileSync(join(dir, '.gitignore'), 'utf8');
  const baseline = readFileSync(join(dir, '.guardrails', 'baseline.json'), 'utf8');
  if (!ignore.includes('node_modules/')) throw new Error('init did not ignore node_modules');
  if (baseline.includes('node_modules/')) throw new Error('baseline recorded files under node_modules');
  if (!check.includes('PASS')) throw new Error(check);
  console.log('quickstart ok');
} finally {
  rmSync(dir, { recursive: true, force: true });
  rmSync(tarball, { force: true });
}
