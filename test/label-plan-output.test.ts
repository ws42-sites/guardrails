import { config, fixture, flagFile, labelRepo, labels, testSource } from './helpers/label-plan.js';
import { expect, it, vi } from 'vitest';
import { planLabels, toMarkdown } from '../src/label-plan.js';
import { inventory } from '../src/inventory.js';

async function cli(args: string[]) {
  vi.resetModules();
  const argv = process.argv;
  process.argv = ['node', 'guardrails', ...args, '--cwd', fixture.cwd];
  const stdout: string[] = [];
  const stderr: string[] = [];
  const log = vi.spyOn(console, 'log').mockImplementation((value: string) => { stdout.push(value); });
  const error = vi.spyOn(console, 'error').mockImplementation((value: string) => { stderr.push(value); });
  let restoreExit: () => void;
  const exited = new Promise<number>((done) => {
    const exit = vi.spyOn(process, 'exit').mockImplementation((code) => { done(Number(code)); return undefined as never; });
    restoreExit = () => exit.mockRestore();
  });
  try {
    await import('../src/cli.js');
    return { status: await exited, stdout: stdout.join('\n'), stderr: stderr.join('\n') };
  } finally {
    process.argv = argv;
    log.mockRestore();
    error.mockRestore();
    restoreExit!();
  }
}

it.each([{ plan: [] }, { plan: [{ label: 'a|b', reason: '<why>\r\n& now\nnext', file: 'a|b.ts', line: 7, status: 'needed' as const },
  { label: 'present', reason: 'Reviewed', file: 'config.json', status: 'already present' as const }] }])('LPLAN-02 renders one escaped advisory summary %j', ({ plan }) => {
  expect(toMarkdown(plan)).toBe('| Label | Why | File:line |\n| --- | --- | --- |\n' + (plan.length
    ? '| a&#124;b (needed) | &lt;why&gt; &amp; now next | a&#124;b.ts:7 |\n| present (already present) | Reviewed | config.json |\n'
    : 'No approval labels needed.\n') + '\nA code owner adds these labels once; Guardrails never adds labels.\n');
});

it.each([
  ['json', ['labels', 'plan', '--base', 'HEAD', '--format', 'json'], false],
  ['markdown', ['labels', 'plan', '--base', 'HEAD', '--format', 'markdown'], false],
  ['bad base', ['labels', 'plan', '--base', 'missing'], true],
  ['bad format', ['labels', 'plan', '--format', 'bad'], true],
  ['bad action', ['labels', 'apply'], true],
  ['extra argument', ['labels', 'plan', 'extra'], true],
] as const)('LPLAN-02 CLI stays read-only and exits zero for %s', async (_, args, advisory) => {
  const r = labelRepo({ '.guardrails/flags.json': flagFile });
  const before = structuredClone(fixture.files);
  const out = await cli([...args, '--labels', labels.flagsResolved]);
  expect(out.status).toBe(0);
  expect(out.stderr.startsWith('labels plan advisory:')).toBe(advisory);
  if (!advisory) {
    const plan = await planLabels(r.ctx('HEAD', [labels.flagsResolved]));
    expect(out.stdout.trim()).toBe((args as readonly string[]).includes('json') ? JSON.stringify(plan, null, 2) : toMarkdown(plan).trim());
    expect(out.stderr).toBe('');
  }
  expect(fixture.files).toEqual(before);
});

it.each(['config', 'flags', 'test'])('LPLAN-02 malformed %s produces an advisory and zero exit', async (kind) => {
  const r = labelRepo();
  const file = kind === 'config' ? 'guardrails.config.json' : kind === 'flags' ? '.guardrails/flags.json' : 'test/broken.test.ts';
  r.write({ [file]: '{' });
  const out = await cli(['labels', 'plan', '--base', 'HEAD']);
  expect(out.status).toBe(0);
  expect(out.stderr.startsWith('labels plan advisory:')).toBe(true);
  expect(out.stdout).toBe('');
  if (kind === 'test') expect(out.stderr).toBe(`labels plan advisory: ${file}: ${inventory('{', '^[A-Z]+-\\d+$').parseError}`);
});

it('LPLAN-02 defaults to the configured remote base and environment labels', async () => {
  const r = labelRepo({ '.guardrails/flags.json': flagFile });
  vi.stubEnv('GITHUB_BASE_REF', '');
  vi.stubEnv('GUARDRAILS_LABELS', ' owner:flags, ');
  try {
    const out = await cli(['labels', 'plan']);
    expect(out.status).toBe(0);
    expect(out.stderr).toBe('');
    expect(out.stdout.trim()).toBe(toMarkdown(await planLabels(r.ctx('HEAD', [labels.flagsResolved]))).trim());
  } finally {
    vi.unstubAllEnvs();
  }
});

it('LPLAN-02 keeps every reason when an approval label is already present', async () => {
  const r = labelRepo();
  r.write({ 'test/a.test.ts': testSource(), 'test/b.test.ts': testSource() });
  const plan = await planLabels(r.ctx('HEAD', [labels.characterization]));
  expect(plan.map(({ file, status, label }) => ({ file, status, label }))).toEqual([
    { file: 'test/a.test.ts', status: 'already present', label: config.labels.characterization },
    { file: 'test/b.test.ts', status: 'already present', label: config.labels.characterization },
  ]);
});
