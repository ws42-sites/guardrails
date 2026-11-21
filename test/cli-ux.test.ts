import { expect, it, vi } from 'vitest';
import { resolve } from 'node:path';
import { repo } from './helpers/repo.js';

async function invoke(args: string[]) {
  vi.resetModules();
  const original = process.argv;
  process.argv = ['node', 'guardrails', ...args];
  const stdout: string[] = [];
  const stderr: string[] = [];
  const log = vi.spyOn(console, 'log').mockImplementation((value: string) => { stdout.push(value); });
  const error = vi.spyOn(console, 'error').mockImplementation((value: string) => { stderr.push(value); });
  let restoreExit: (() => void) | undefined;
  const exited = new Promise<number>((done) => {
    const exit = vi.spyOn(process, 'exit').mockImplementation((code) => { done(Number(code)); return undefined as never; });
    restoreExit = () => exit.mockRestore();
  });
  try {
    await import('../src/cli.js');
    return { status: await exited, stdout: stdout.map((line) => `${line}\n`).join(''), stderr: stderr.map((line) => `${line}\n`).join('') };
  } finally {
    process.argv = original;
    log.mockRestore();
    error.mockRestore();
    restoreExit?.();
  }
}
const subcommands = ['init runbook', 'provenance record', 'provenance verify', 'ledger add', 'ledger import', 'feedback draft', 'telemetry status', 'labels plan', 'journeys generate-tests', 'journeys scaffold', 'journeys run', 'journeys list', 'journeys draft', 'journeys import', 'journeys approve'];

it.each(subcommands)('REPORT-01 %s has help for both spellings', async (command) => {
  const run = (flag: string) => invoke([...command.split(' '), flag, '--cwd', resolve('.task/does-not-exist')]);
  const long = await run('--help');
  const short = await run('-h');
  expect(long.stdout.split('\n').filter((line) => line.startsWith(`  ${command} `)).length).toBe(1);
  expect({ exit: long.status, stderr: long.stderr, first: long.stdout.split('\n')[0] }).toEqual({ exit: 0, stderr: '', first: `Usage: guardrails ${command} [options]` });
  expect({ exit: short.status, stdout: short.stdout, stderr: short.stderr }).toEqual({ exit: 0, stdout: long.stdout, stderr: '' });
});

it.each([
  [['unknown'], 'Unknown command: unknown.', ''],
  [['check', 'extra'], 'Unexpected argument: extra.', 'check'],
  [['explain', 'NOT-A-RULE'], 'Unknown rule: NOT-A-RULE.', 'explain'],
  [['explain', 'DOC-1', 'extra'], 'Unexpected argument: extra.', 'explain'],
  [['check-links'], 'check-links: pass one built directory.', 'check-links'],
  [['validate-test'], 'validate-test: a spec ID is required.', 'validate-test'],
  [['telemetry'], 'telemetry: a subcommand is required.', 'telemetry'],
  [['telemetry', 'bad'], 'Unknown telemetry subcommand: bad.', 'telemetry'],
  [['provenance', 'record'], 'provenance: record requires --report; verify requires --head.', 'provenance'],
  [['journeys', 'import'], 'journeys import: a JSON file is required.', 'journeys'],
  [['journeys', 'run'], 'journeys run: --base-url is required.', 'journeys'],
  [['journeys', 'approve'], 'journeys approve: a journey ID is required.', 'journeys'],
  [['holdout'], 'holdout: --dir is required.', 'holdout'],
  [['flag'], 'flag: a spec ID and reason are required.', 'flag'],
  [['ledger', 'add'], 'ledger add: <kind> and --pr are required.', 'ledger'],
  [['ledger', 'add', 'bad', '--pr', '1'], 'ledger add: kind must be escaped-bug or false-escalation.', 'ledger'],
  [['ledger', 'add', 'escaped-bug', '--pr', '1'], 'ledger add escaped-bug: --issue is required.', 'ledger'],
  [['ledger', 'add', 'false-escalation', '--pr', '1'], 'ledger add false-escalation: --rule and --label are required.', 'ledger'],
  [['ledger', 'add', 'false-escalation', '--pr', '1', '--rule', 'DOC-1'], 'ledger add false-escalation: --rule and --label are required.', 'ledger'],
  [['ledger', 'import'], 'ledger: add <kind> --pr N | import --events file.', 'ledger'],
])('MODE-01 usage error %j is concise and goes only to stderr', async (args, message, command) => {
  const result = await invoke(args);
  expect({ exit: result.status, stdout: result.stdout, stderr: result.stderr }).toEqual({ exit: 2, stdout: '', stderr: `${message}\nRun "guardrails ${command ? `${command} ` : ''}--help" for usage.\n` });
});

it.each([['check', '--not-a-flag'], ['check', '--cwd']])('MODE-01 parser errors %j include a help hint', async (...args) => {
  const result = await invoke(args);
  const message = args[1] === '--cwd' ? "Option '--cwd <value>' argument missing" : "Unknown option '--not-a-flag'. To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- \"--not-a-flag\"";
  expect({ exit: result.status, stdout: result.stdout, stderr: result.stderr }).toEqual({ exit: 2, stdout: '', stderr: `${message}\nRun "guardrails --help" for usage.\n` });
});


const commands = ['init', 'check-links', 'smoke', 'secrets-check', 'affected', 'check-tests', 'shard', 'check', 'review', 'report', 'provenance', 'baseline', 'specs', 'explain', 'ledger', 'evaluate', 'feedback', 'validate-test', 'telemetry', 'flag', 'labels', 'mutants', 'hook-guard', 'holdout', 'journeys'];

it.each(commands)('REPORT-01 %s help exits successfully without running the command', async (command) => {
  const args = [command, '--help', '--cwd', resolve('.task/does-not-exist')];
  const long = await invoke(args);
  const short = await invoke(args.map((arg) => arg === '--help' ? '-h' : arg));
  expect({ exit: long.status, error: long.stderr, usage: long.stdout.split('\n')[0], help: long.stdout.includes('-h, --help') }).toEqual({ exit: 0, error: '', usage: `Usage: guardrails ${command} [options]`, help: true });
  expect({ exit: short.status, stdout: short.stdout, stderr: short.stderr }).toEqual({ exit: 0, stdout: long.stdout, stderr: '' });
});

it.each([{ args: [] }, { args: ['--help'] }, { args: ['-h'] }])('REPORT-01 top-level help %j uses stdout and exits zero', async ({ args }) => {
  const result = await invoke(args);
  expect({ exit: result.status, stderr: result.stderr, first: result.stdout.split('\n')[0], help: result.stdout.includes('-h, --help') }).toEqual({ exit: 0, stderr: '', first: 'guardrails <command> [options]', help: true });
});
