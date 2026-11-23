import { describe, expect, it } from 'vitest';
import { checkRunbook } from '../src/checks/runbook.js';
import { fingerprint, lineCount } from '../src/util.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const RB = 'RUNBOOK.md';
const WF = '.github/workflows/ship.yml';
const SECTIONS = ['Deploy', 'Rollback', 'Smoke', 'Secrets rotation', 'Incident first steps'];
const SCRIPTS = {
  deploy: 'wrangler deploy',
  'deploy:preview': 'wrangler versions upload',
  rollback: 'echo pin the previous release',
  smoke: 'node smoke.mjs',
  'secrets:check': 'node secrets.mjs',
};
const pkg = (scripts: Record<string, string>) => JSON.stringify({ scripts });
const base = (over: Record<string, unknown> = {}) => ({ ...BASE_CONFIG, runbook: { enabled: true, ...over } });
const ROLLBACK_NA = { rollback: 'Pages keeps no versions we can promote' };
const naRollback = base({ na: ROLLBACK_NA });
const doc = (lines: string[]) => lines.join('\n');
const padTo = (md: string, n: number) => {
  const lines = md.split('\n');
  return [...lines, ...Array(n - lines.length).fill('padding')].join('\n');
};

type Hit = { rule: string; file: string; line: number | null; message: string; fingerprint: string };
const view = (v: { rule: string; file: string; line?: number; message: string; fingerprint: string }): Hit => ({
  rule: v.rule, file: v.file, line: v.line ?? null, message: v.message, fingerprint: v.fingerprint,
});
const run = (files: Record<string, string>, config: object = base(), list?: string[], pad?: number) => {
  const written = { ...files };
  if (pad && written[RB]) written[RB] = padTo(written[RB], pad);
  const r = repo({ 'package.json': pkg(SCRIPTS), ...written }, config);
  const got = (list ? checkRunbook(r.ctx(), list) : checkRunbook(r.ctx())).violations;
  return got.map(view);
};

const cmd = (title: string, script: string | undefined, line: number): Hit => ({
  rule: 'RUN-1', file: RB, line,
  message: `"${title}" must point to a command (\`npm run ${script ?? 'smoke'}\`)${script ? ', or say n/a when runbook.na lists it' : ''}.`,
  fingerprint: fingerprint('RUN-1', RB, `cmd:${title}`),
});
const click = (title: string, prose: string, line: number): Hit => ({
  rule: 'RUN-1', file: RB, line,
  message: `"${title}" has a manual click-path ("${prose.trim().slice(0, 60)}"). Script it and point to the command.`,
  fingerprint: fingerprint('RUN-1', RB, `click:${title}:${prose.trim()}`),
});
const miss = (names: string[]): Hit => ({
  rule: 'RUN-1', file: RB, line: null,
  message: `missing section(s): ${names.join(', ')}. Sections: ${SECTIONS.join(', ')}.`,
  fingerprint: fingerprint('RUN-1', RB, 'sections'),
});
const order: Hit = {
  rule: 'RUN-1', file: RB, line: null,
  message: `sections out of order. Use: ${SECTIONS.join(', ')}.`,
  fingerprint: fingerprint('RUN-1', RB, 'order'),
};
const missingFile: Hit = {
  rule: 'RUN-1', file: RB, line: null,
  message: `missing ${RB}. Run "guardrails init runbook".`,
  fingerprint: fingerprint('RUN-1', RB, 'missing'),
};
const cap = (n: number): Hit => ({
  rule: 'RUN-4', file: RB, line: null,
  message: `${n} lines (cap 150). Keep commands, cut prose.`,
  fingerprint: fingerprint('RUN-4', RB, 'cap'),
});
const missingScript = (s: string): Hit => ({
  rule: 'RUN-2', file: 'package.json', line: null,
  message: `missing script "${s}". Wire it to the real command or add runbook.na["${s}"] with the reason.`,
  fingerprint: fingerprint('RUN-2', 'package.json', `script:${s}`),
});
const ref = (s: string): Hit => ({
  rule: 'RUN-2', file: RB, line: null,
  message: `references \`npm run ${s}\` but package.json has no "${s}" script.`,
  fingerprint: fingerprint('RUN-2', RB, `ref:${s}`),
});
const smokeHit = (file: string, line: number): Hit => ({
  rule: 'RUN-3', file, line,
  message: 'deploys without running `npm run smoke` afterwards.',
  fingerprint: fingerprint('RUN-3', file, 'smoke'),
});
const stray = (file: string): Hit => ({
  rule: 'RUN-4', file, line: null,
  message: `runbook/deploy doc outside ${RB}. Fold it into ${RB} and delete it.`,
  fingerprint: fingerprint('RUN-4', file, 'stray'),
});

const okDoc = doc([
  'note ## Deploy',
  '# Runbook',
  '##  Deploy',
  '`npm run deploy`',
  '## Rollback ##',
  'N/A: host keeps the previous release',
  '## Smoke',
  '```',
  'npm run smoke',
  '```',
  '## Secrets rotation',
  '`npm run secrets:check`',
  '## Incident first steps',
  '`npm run smoke`',
  '```',
  'click the dashboard',
  '```',
  ' ```',
  'click the dashboard',
  ' ```click the dashboard',
  '```click the dashboard',
  '```',
]);
const clean = doc([
  '# Runbook',
  '## Deploy',
  '`npm run deploy`',
  '## Rollback',
  '`npm run rollback`',
  '## Smoke',
  '`npm run smoke`',
  '## Secrets rotation',
  '`npm run secrets:check`',
  '## Incident first steps',
  '`npm run smoke`',
]);
const linked = doc([
  '# Runbook',
  '## Deploy',
  '`npm run deploy`',
  '## Rollback',
  '`npm run rollback`',
  '## Smoke',
  '`npm run smoke`',
  '## Secrets rotation',
  '`npm run secrets:check`',
  '## Incident first steps',
  '`npm run purge`',
  'then `npm run purge` again',
]);
const longTap = '  tap the portal and then keep going until this click path exceeds sixty characters';
const openProse = 'open the console ` later';
const clickDoc = doc([
  '# Runbook',
  '## Deploy',
  '`npm run deploy`',
  'login to the dashboard',
  '## Rollback',
  '`npm run rollback`',
  '',
  'go to the dashboard',
  '## Smoke',
  '`npm run smoke`',
  'cli`xxxx`ck the dashboard',
  '## Secrets rotation',
  '`npm run secrets:check`',
  'Run `click the dashboard` afterwards',
  '## Incident first steps',
  '`npm run smoke`',
  '```',
  'click the dashboard',
  '```',
  'open the console ``` later',
  longTap,
]);

describe('RUN-01 sections, commands, click-paths, length and scripts', () => {
  it.each([
    ['accepts two-space headings, fenced commands, n/a and ignored fences', { [RB]: okDoc }, naRollback, undefined, undefined, []],
    ['names every missing section', {
      [RB]: doc(['# Runbook', '## Deploy', '`npm run deploy`', '## Rollback', '`npm run rollback`', '## Smoke', '`npm run smoke`']),
    }, base(), undefined, undefined, [miss(['Secrets rotation', 'Incident first steps'])]],
    ['rejects sections out of order', {
      [RB]: doc(['# Runbook', '## Smoke', '`npm run smoke`', '## Deploy', '`npm run deploy`', '## Rollback', '`npm run rollback`', '## Secrets rotation', '`npm run secrets:check`', '## Incident first steps', '`npm run smoke`']),
    }, base(), undefined, undefined, [order]],
    ['quotes the script each empty section must point at', {
      [RB]: doc(['# Runbook', '## Deploy', 'words', '## Rollback', 'words', '## Smoke', 'words', '## Secrets rotation', 'words', '## Incident first steps', 'words']),
    }, base(), undefined, undefined, [cmd('Deploy', 'deploy', 2), cmd('Rollback', 'rollback', 4), cmd('Smoke', 'smoke', 6), cmd('Secrets rotation', 'secrets:check', 8), cmd('Incident first steps', undefined, 10)]],
    ['accepts n/a only for a listed script that says so', {
      [RB]: doc(['# Runbook', '## Deploy', 'n/a we ship by hand', '## Rollback', 'N/A for this package', '## Smoke', 'still required here', '## Secrets rotation', '`npm run secrets:check`', '## Incident first steps', 'n/a see the pager']),
    }, base({ na: { ...ROLLBACK_NA, smoke: 'No hosted URL to probe in this library' } }), undefined, undefined, [cmd('Deploy', 'deploy', 2), cmd('Smoke', 'smoke', 6), cmd('Incident first steps', undefined, 10)]],
    ['reports click-paths with the stripped line and prose index', { [RB]: clickDoc }, base(), undefined, undefined, [
      click('Deploy', 'login to the dashboard', 4),
      click('Rollback', 'go to the dashboard', 8),
      click('Smoke', 'click the dashboard', 11),
      click('Incident first steps', openProse, 17),
      click('Incident first steps', longTap, 18),
    ]],
    ['reports a missing runbook', {}, base(), undefined, undefined, [missingFile]],
    ['reports each missing standard script and each unknown reference', { 'package.json': pkg({}), [RB]: linked }, base(), undefined, undefined, [
      ...['deploy', 'deploy:preview', 'rollback', 'smoke', 'secrets:check'].map(missingScript),
      ...['deploy', 'rollback', 'smoke', 'secrets:check', 'purge', 'purge'].map(ref),
    ]],
    ['reports every reference that package.json does not define', { [RB]: linked }, base(), undefined, undefined, [ref('purge'), ref('purge')]],
    ['allows a runbook of exactly 150 lines', { [RB]: okDoc }, naRollback, undefined, 150, []],
    ['flags line 151 against the 150 line cap', { [RB]: okDoc }, naRollback, undefined, 151, [cap(151)]],
  ])('%s', (_name, files, config, list, pad, expected) => {
    if (pad && files[RB]) expect(lineCount(padTo(files[RB], pad))).toBe(pad);
    expect(run(files, config, list, pad)).toEqual(expected);
  });
});

describe('RUN-02 workflows call the deploy script then smoke, and stray docs stay out', () => {
  const rows: [string, string | null][] = [
    ['# cdk deploy', null],
    ['#cdk deploy', null],
    ['  # wrangler deploy', null],
    ['cdk deploy # now', 'cdk deploy'],
    ['wrangler deploy', 'wrangler deploy'],
    ['wrangler  pages deploy', 'wrangler  pages deploy'],
    ['wrangler pages  deploy', 'wrangler pages  deploy'],
    ['wrangler.js deploy', 'wrangler.js deploy'],
    ['cdk deploy', 'cdk deploy'],
    ['cdk  deploy', 'cdk  deploy'],
    ['vercel deploy', 'vercel deploy'],
    ['vercel  deploy', 'vercel  deploy'],
    ['vercel --prod', 'vercel --prod'],
    ['netlify deploy', 'netlify deploy'],
    ['netlify  deploy', 'netlify  deploy'],
    ['fly deploy', 'fly deploy'],
    ['fly  deploy', 'fly  deploy'],
    ['flyctl deploy', 'flyctl deploy'],
    ['serverless deploy', 'serverless deploy'],
    ['serverless  deploy', 'serverless  deploy'],
    ['sls deploy', 'sls deploy'],
    ['sls  deploy', 'sls  deploy'],
    ['sam deploy', 'sam deploy'],
    ['sam  deploy', 'sam  deploy'],
    ['firebase deploy', 'firebase deploy'],
    ['firebase  deploy', 'firebase  deploy'],
    ['aws s3 sync', 'aws s3 sync'],
    ['aws  s3 sync', 'aws  s3 sync'],
    ['aws s3  sync', 'aws s3  sync'],
    ['render deploy', 'render deploy'],
    ['render  deploy', 'render  deploy'],
    ['uses:cloudflare/wrangler-action@v3', 'uses:cloudflare/wrangler-action@v3'],
    ['uses:  cloudflare/pages-action@v3', 'uses:  cloudflare/pages-action@v3'],
    ['https://api.render.com/deploy/x', 'https://api.render.com/deploy/x'],
    ['  cdk deploy', '  cdk deploy'],
    [`wrangler deploy ${'y'.repeat(90)}`, `wrangler deploy ${'y'.repeat(90)}`],
    ['notcdk deploy', null],
    ['cdk synth', null],
    ['wrangler dev', null],
    ['npx wrangler deploy --dry-run', null],
    ['npm run deploy && echo shipped', null],
    ['npm run smoke && true', null],
  ];
  const inlineHits = rows.flatMap(([, code], i) => code === null ? [] : [{
    rule: 'RUN-3', file: WF, line: i + 1,
    message: `inline deploy command "${code.trim().slice(0, 80)}". Call \`npm run deploy\` (or deploy:preview) instead.`,
    fingerprint: fingerprint('RUN-3', WF, `inline:${code.trim()}`),
  }]);
  const flagged = ['DEPLOY.md', 'docs/deploy.md', 'docs/deployment.md', 'docs/deploy.mdx', 'ops/runbook-extra.md', 'docs/my-playbook-v2.md', 'Playbook.mdx', 'nested/rollback-plan.md', 'docs/Deploy.MD', 'ops/chat-runbook.md'];
  const quiet = ['docs/deploy.md.bak', 'my-deploy.md', 'features/FEAT-1-managed-deployer.md', 'docs/readme.md', 'node_modules/pkg/RUNBOOK.md', 'apps/web/node_modules/lib/deploy.md', 'legacy/ROLLBACK.md'];

  it.each([
    ['flags each inline deploy and still counts a later compound smoke', { [RB]: clean, [WF]: rows.map(([raw]) => raw).join('\n') }, base(), undefined, inlineHits],
    ['ignores a workflow that never deploys', { [RB]: clean, [WF]: 'echo ok\n' }, base(), undefined, []],
    ['accepts smoke on the line after a deploy that starts the file', { [RB]: clean, [WF]: 'npm run deploy\nnpm run smoke\n' }, base(), undefined, []],
    ['flags a first-line deploy with no smoke', { [RB]: clean, [WF]: 'npm run deploy\n' }, base(), undefined, [smokeHit(WF, 1)]],
    ['flags a trailed deploy with no smoke', { [RB]: clean, [WF]: 'npm run deploy \n' }, base(), undefined, [smokeHit(WF, 1)]],
    ['does not treat npm run deployment as deploy', { [RB]: clean, [WF]: 'npm run deployment\n' }, base(), undefined, []],
    ['does not treat npm run deploy-extra as deploy', { [RB]: clean, [WF]: 'npm run deploy-extra\n' }, base(), undefined, []],
    ['does not treat npm run deploy:preview:extra as deploy', { [RB]: clean, [WF]: 'npm run deploy:preview:extra\n' }, base(), undefined, []],
    ['does not treat npm run smoke-now as smoke', { [RB]: clean, [WF]: 'npm run deploy\nnpm run smoke-now\n' }, base(), undefined, [smokeHit(WF, 1)]],
    ['accepts npm run smoke when more text follows', { [RB]: clean, [WF]: 'npm run deploy\nnpm run smoke && true\n' }, base(), undefined, []],
    ['ignores a dry-run', { [RB]: clean, [WF]: 'npx wrangler deploy --dry-run\n' }, base(), undefined, []],
    ['requires smoke after deploy when smoke ran only before it', { [RB]: clean, [WF]: 'npm run smoke\nnpm run deploy\n' }, base(), undefined, [smokeHit(WF, 2)]],
    ['accepts deploy:preview followed by a trailed smoke', { [RB]: clean, [WF]: 'npm run deploy:preview\nnpm run smoke \n' }, base(), undefined, []],
    ['flags stray runbook docs and skips allowed, suffixed and node_modules paths', { [RB]: okDoc }, base({ na: ROLLBACK_NA, allowDocs: ['legacy/**'] }), [RB, 'package.json', ...flagged, ...quiet], flagged.map(stray)],
  ])('%s', (_name, files, config, list, expected) => {
    expect(run(files, config, list)).toEqual(expected);
  });
});
