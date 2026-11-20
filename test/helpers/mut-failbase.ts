import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach } from 'vitest';
import { BASE_CONFIG, repo, type Repo } from './repo.js';

export interface Condition {
  file: string;
  contains: string;
}

export interface FakeAssertion {
  title: string;
  pass?: boolean;
  allPresent?: Condition[];
  anyPresent?: Condition[];
  onlyWhen?: Condition;
}

export interface FakeFilePlan {
  nameFormat?: 'abs' | 'rel' | 'winRel';
  assertions: FakeAssertion[];
}

export interface FakeRunnerPlan {
  mode?: 'normal' | 'missingOutput' | 'invalidJson' | 'emptyObject' | 'omitAssertions';
  files?: Record<string, FakeFilePlan>;
}

export interface RunnerInvocation {
  cwd: string;
  args: string[];
  clockEnv: string | undefined;
  hasNodeModules: boolean;
  hasNodeModulesEntry: boolean;
  nodeModulesMarker: string | null;
  configText: string | null;
}

export interface FakeRunner {
  command: string;
  setPlan(plan: FakeRunnerPlan): void;
  invocations(): RunnerInvocation[];
  clearLog(): void;
}

const runnerDirs: string[] = [];
afterEach(() => {
  while (runnerDirs.length) rmSync(runnerDirs.pop() as string, { recursive: true, force: true });
});

const RUNNER_SOURCE = `import { appendFileSync, existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [stateDir, ...args] = process.argv.slice(2);
const outArg = args.find((a) => a.startsWith('--outputFile='));
const outFile = outArg ? outArg.slice('--outputFile='.length) : '';
const files = args.filter((a) => !a.startsWith('--'));
const cwd = process.cwd();
const plan = JSON.parse(readFileSync(join(stateDir, 'plan.json'), 'utf8'));
const nmMarker = join(cwd, 'node_modules', 'pkg', 'ok.txt');
const cfgPath = join(cwd, 'guardrails.config.json');

appendFileSync(join(stateDir, 'log.jsonl'), JSON.stringify({
  cwd,
  args,
  clockEnv: process.env.GUARDRAILS_CLOCK_SHIFT_YEARS,
  hasNodeModules: existsSync(join(cwd, 'node_modules')),
  hasNodeModulesEntry: readdirSync(cwd).includes('node_modules'),
  nodeModulesMarker: existsSync(nmMarker) ? readFileSync(nmMarker, 'utf8') : null,
  configText: existsSync(cfgPath) ? readFileSync(cfgPath, 'utf8') : null,
}) + '\\n');

if (plan.mode === 'invalidJson') {
  writeFileSync(outFile, '{corrupt-json');
} else if (plan.mode === 'emptyObject') {
  writeFileSync(outFile, '{}');
} else if (plan.mode === 'omitAssertions') {
  writeFileSync(outFile, JSON.stringify({ testResults: files.map((f) => ({ name: join(cwd, f) })) }));
} else if (plan.mode !== 'missingOutput') {
  const testResults = files.map((f) => {
    const spec = (plan.files && plan.files[f]) || { assertions: [] };
    const name = spec.nameFormat === 'rel'
      ? f
      : spec.nameFormat === 'winRel'
        ? f.split('/').join('\\\\')
        : join(cwd, f);
    const hasCond = (c) => {
      const p = join(cwd, c.file);
      return existsSync(p) && readFileSync(p, 'utf8').includes(c.contains);
    };
    const assertionResults = (spec.assertions || [])
      .filter((a) => !a.onlyWhen || hasCond(a.onlyWhen))
      .map((a) => {
        let passed = Boolean(a.pass);
        if (a.allPresent) passed = a.allPresent.every(hasCond);
        else if (a.anyPresent) passed = a.anyPresent.some(hasCond);
        return { title: a.title, status: passed ? 'passed' : 'failed' };
      });
    return { name, assertionResults };
  });
  writeFileSync(outFile, JSON.stringify({ testResults }));
}
`;

export function createFakeRunner(initial: FakeRunnerPlan = { mode: 'normal', files: {} }): FakeRunner {
  const dir = mkdtempSync(join(tmpdir(), 'guardrails-runner-'));
  runnerDirs.push(dir);
  const script = join(dir, 'runner.mjs');
  const planPath = join(dir, 'plan.json');
  const logPath = join(dir, 'log.jsonl');
  writeFileSync(script, RUNNER_SOURCE);
  writeFileSync(planPath, JSON.stringify(initial));
  const posix = (p: string) => p.split('\\').join('/');
  return {
    command: `node ${JSON.stringify(posix(script))} ${JSON.stringify(posix(dir))}`,
    setPlan(plan) {
      writeFileSync(planPath, JSON.stringify(plan));
    },
    invocations() {
      if (!existsSync(logPath)) return [];
      return readFileSync(logPath, 'utf8')
        .split(/\r?\n/)
        .filter(Boolean)
        .map((line) => JSON.parse(line) as RunnerInvocation);
    },
    clearLog() {
      if (existsSync(logPath)) rmSync(logPath);
    },
  };
}

export function failbaseConfig(command: string, extraSpecs: Record<string, { tier: 'T1' | 'T2' | 'T3'; behaviour: string; regressionGuard?: boolean }> = {}) {
  return {
    ...BASE_CONFIG,
    specs: { items: { ...BASE_CONFIG.specs.items, ...extraSpecs } },
    tests: { command, helperGlobs: ['test/helpers/**'] },
  };
}

export function testFile(...cases: [title: string, body?: string][]): string {
  return `import { expect, it } from 'vitest';\n${cases.map(([t, b = 'expect(1).toBe(1);']) => `it(${JSON.stringify(t)}, () => { ${b} });`).join('\n')}\n`;
}

export function gitRun(cwd: string, ...args: string[]): string {
  return execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false', ...args], {
    cwd,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
}

export function worktreeCount(cwd: string): number {
  return gitRun(cwd, 'worktree', 'list', '--porcelain').split(/\r?\n/).filter((l) => l.startsWith('worktree ')).length;
}

export function initRepo(files: Record<string, string>, config: object): Repo {
  return repo({ '.gitignore': 'node_modules\n', ...files }, config).commit('base');
}
