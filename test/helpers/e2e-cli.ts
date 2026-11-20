import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach } from 'vitest';
import type { Report } from '../../src/types.js';

export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
export const CLI_PATH = join(REPO_ROOT, 'dist', 'cli.js');
export const NODE_MODULES_PATH = join(REPO_ROOT, 'node_modules');
export const VITEST_ENTRY_PATH = join(NODE_MODULES_PATH, 'vitest', 'vitest.mjs');
export const VITEST_INDEX_PATH = join(NODE_MODULES_PATH, 'vitest', 'dist', 'index.js');
export const VITEST_RUNNER_FILE = 'vitest-runner.mjs';
export const VITEST_CMD = `${JSON.stringify(process.execPath)} ./${VITEST_RUNNER_FILE}`;

export function vitestRunnerSource(): string {
  return [
    "import { spawnSync } from 'node:child_process';",
    "import { existsSync, readFileSync, writeFileSync } from 'node:fs';",
    "import { resolve } from 'node:path';",
    'const args = process.argv.slice(2);',
    `const res = spawnSync(process.execPath, [${JSON.stringify(VITEST_ENTRY_PATH)}, 'run', '--pool=forks', '--no-file-parallelism', ...args], { stdio: 'inherit' });`,
    "const outFile = args.find((a) => a.startsWith('--outputFile='))?.slice('--outputFile='.length);",
    'if (outFile && existsSync(outFile)) {',
    "  const data = JSON.parse(readFileSync(outFile, 'utf8'));",
    '  for (const tr of data.testResults ?? []) tr.name = resolve(tr.name);',
    '  writeFileSync(outFile, JSON.stringify(data));',
    '}',
    'process.exit(res.status ?? 1);',
    '',
  ].join('\n');
}

export function e2eVitestConfigSource(): string {
  return [
    'export default {',
    "  cacheDir: './.vite',",
    `  resolve: { alias: { vitest: ${JSON.stringify(VITEST_INDEX_PATH)} } },`,
    "  test: { setupFiles: ['./vitest.setup.ts'], server: { deps: { external: [/node_modules/] } } },",
    '};',
    '',
  ].join('\n');
}

const STRIP_ENV = [
  'GITHUB_BASE_REF',
  'GUARDRAILS_LABELS',
  'GUARDRAILS_SPECS',
  'GUARDRAILS_AGENT',
  'GUARDRAILS_ALLOW_TEST_EDITS',
  'CLAUDE_PROJECT_DIR',
  'GEMINI_PROJECT_DIR',
  'GIT_DIR',
  'GIT_WORK_TREE',
  'GIT_INDEX_FILE',
  'GEMINI_API_KEY',
  'OPENAI_API_KEY',
] as const;

const GIT_ENV: NodeJS.ProcessEnv = {
  ...process.env,
  GIT_AUTHOR_DATE: '2026-01-01T00:00:00Z',
  GIT_COMMITTER_DATE: '2026-01-01T00:00:00Z',
};

const createdDirs: string[] = [];
afterEach(() => {
  while (createdDirs.length) {
    rmSync(createdDirs.pop() as string, { recursive: true, force: true });
  }
});

export interface CliResult {
  status: number;
  stdout: string;
  stderr: string;
}

export interface RunCliOptions {
  input?: string;
  env?: Record<string, string | undefined>;
  cliPath?: string;
}

export function ensureCliBuilt(cliPath = CLI_PATH): void {
  if (!existsSync(cliPath)) {
    throw new Error(`Built guardrails CLI not found at ${cliPath}. Run "npm run build" before running e2e tests.`);
  }
}

export function runCli(cwd: string, args: string[], opts: RunCliOptions = {}): CliResult {
  const cliPath = opts.cliPath ?? CLI_PATH;
  ensureCliBuilt(cliPath);
  const env: NodeJS.ProcessEnv = { ...process.env, GUARDRAILS_OFFLINE: '1' };
  for (const key of STRIP_ENV) delete env[key];
  for (const [k, v] of Object.entries(opts.env ?? {})) {
    if (v === undefined) delete env[k];
    else env[k] = v;
  }
  const res = spawnSync(process.execPath, [cliPath, ...args], {
    cwd,
    input: opts.input,
    encoding: 'utf8',
    env,
    maxBuffer: 64 << 20,
  });
  if (res.error) throw res.error;
  return {
    status: res.status ?? 1,
    stdout: res.stdout.replace(/\r\n/g, '\n'),
    stderr: res.stderr.replace(/\r\n/g, '\n'),
  };
}

export function checkJson(cwd: string, args: string[] = [], env?: Record<string, string | undefined>): { status: number; report: Report } {
  const res = runCli(cwd, ['check', '--format', 'json', ...args], { env });
  return { status: res.status, report: JSON.parse(res.stdout) as Report };
}

export const E2E_BASE_CONFIG = {
  version: 1,
  sourceRoots: ['src'],
  tiers: { T1: ['src/core/**'], T2: ['src/lib/**'], T3: [] },
  specs: {
    items: {
      'CORE-01': { tier: 'T1', behaviour: 'core adds numbers' },
      'LIB-01': { tier: 'T2', behaviour: 'lib formats text' },
    },
  },
} as const;

export const E2E_PROJECT_FILES: Record<string, string> = {
  'AGENTS.md': '# Agents\n',
  '.github/CODEOWNERS': '/guardrails.config.json @owner\n/.github/ @owner\n/.guardrails/ @owner\n',
  'vitest.config.ts': "export default { test: { setupFiles: ['./vitest.setup.ts'] } };\n",
  'vitest.setup.ts': "import { beforeEach, expect } from 'vitest';\nbeforeEach(() => { expect.hasAssertions(); });\n",
};

export interface E2eRepo {
  dir: string;
  write(files: Record<string, string>): E2eRepo;
  remove(...files: string[]): E2eRepo;
  read(file: string): string;
  exists(file: string): boolean;
  commit(msg?: string): E2eRepo;
  branch(name: string): E2eRepo;
}

export function e2eRepo(files: Record<string, string> = {}, config: object | null = E2E_BASE_CONFIG): E2eRepo {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), 'guardrails-e2e-')));
  createdDirs.push(dir);
  const gitEnv = { ...GIT_ENV };
  delete gitEnv.GIT_DIR;
  delete gitEnv.GIT_WORK_TREE;
  delete gitEnv.GIT_INDEX_FILE;
  const git = (...args: string[]) =>
    execFileSync('git', ['-c', 'user.name=e2e', '-c', 'user.email=e2e@example.test', '-c', 'commit.gpgsign=false', ...args], {
      cwd: dir,
      env: gitEnv,
      stdio: 'pipe',
    });
  git('init', '-q', '-b', 'main');

  const r: E2eRepo = {
    dir,
    write(more) {
      for (const [rel, body] of Object.entries(more)) {
        const target = join(dir, rel);
        mkdirSync(dirname(target), { recursive: true });
        writeFileSync(target, body);
      }
      return r;
    },
    remove(...rels) {
      for (const rel of rels) rmSync(join(dir, rel), { force: true });
      return r;
    },
    read(rel) {
      return readFileSync(join(dir, rel), 'utf8').replace(/\r\n/g, '\n');
    },
    exists(rel) {
      return existsSync(join(dir, rel));
    },
    commit(msg = 'commit') {
      git('add', '-A');
      git('commit', '-q', '--allow-empty', '-m', msg);
      return r;
    },
    branch(name) {
      git('checkout', '-q', '-b', name);
      return r;
    },
  };

  if (config !== null) {
    r.write({
      'guardrails.config.json': `${JSON.stringify(config, null, 2)}\n`,
      ...E2E_PROJECT_FILES,
    });
  }
  r.write(files);
  return r;
}
