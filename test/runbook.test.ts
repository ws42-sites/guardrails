import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { checkRunbook } from '../src/checks/runbook.js';
import { initRunbook } from '../src/init.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const ON = { ...BASE_CONFIG, runbook: { enabled: true, na: { rollback: 'Pages keeps no versions we can promote' } } };
const SCRIPTS = { deploy: 'wrangler deploy', 'deploy:preview': 'wrangler versions upload', smoke: 'node smoke.mjs', 'secrets:check': 'node secrets.mjs' };
const pkg = (scripts: Record<string, string>) => JSON.stringify({ scripts });
const GOOD = `# Runbook\n## Deploy\n\`npm run deploy\`\n## Rollback\nn/a: see runbook.na\n## Smoke\n\`npm run smoke\`\n## Secrets rotation\n\`npm run secrets:check\`\n## Incident first steps\n1. \`npm run smoke\`\n`;
const found = (r: ReturnType<typeof repo>) => checkRunbook(r.ctx()).violations.map((v) => `${v.rule}:${v.message.split(/[.(]/)[0]}`);

describe('RUN-01 RUNBOOK.md has the standard sections, commands and scripts', () => {
  it('passes a conforming runbook and is off until enabled', () => {
    expect(found(repo({ 'RUNBOOK.md': GOOD, 'package.json': pkg(SCRIPTS) }, ON))).toEqual([]);
    expect(checkRunbook(repo({}).ctx()).violations).toEqual([]);
  });

  it.each([
    ['missing file', {}, SCRIPTS, ['RUN-1:missing RUNBOOK']],
    ['order, click-path, missing script, unknown ref, length', {
      'RUNBOOK.md': GOOD.replace('## Smoke\n`npm run smoke`', '## Smoke\nClick Deployments in the Cloudflare dashboard').replace('## Deploy', '## Secrets rotation\n`npm run secrets:check`\n## Deploy').replace(/## Secrets rotation\n`npm run secrets:check`\n## Incident/, '## Incident') + `\`npm run purge\`\n${'x\n'.repeat(150)}`,
    }, { ...SCRIPTS, smoke: undefined }, [
      'RUN-2:missing script "smoke"', 'RUN-4:162 lines ', 'RUN-1:sections out of order', 'RUN-1:"Smoke" must point to a command ', 'RUN-1:"Smoke" has a manual click-path ', 'RUN-2:references `npm run smoke` but package', 'RUN-2:references `npm run purge` but package',
    ]],
  ])('flags %s', (_name, files, scripts, expected) => {
    const r = repo({ 'package.json': pkg(JSON.parse(JSON.stringify(scripts))), ...files }, ON);
    expect(found(r)).toEqual(expected);
  });
});

describe('RUN-02 deploy workflows call npm run deploy then smoke; one runbook', () => {
  it('flags inline deploys, missing smoke and stray runbook docs', () => {
    const r = repo({
      'RUNBOOK.md': GOOD, 'package.json': pkg(SCRIPTS), 'docs/deploy-guide.md': 'x', 'legacy/ROLLBACK.md': 'x',
      '.github/workflows/ok.yml': 'jobs:\n  d:\n    steps:\n      - run: npm run deploy\n      - run: npm run smoke\n',
      '.github/workflows/bad.yml': 'jobs:\n  d:\n    steps:\n      - uses: cloudflare/wrangler-action@v3\n      - run: npx wrangler pages deploy dist # ship\n      - run: npm run smoke\n      - run: npm run deploy:preview\n      - run: node node_modules/wrangler/bin/wrangler.js deploy --dry-run --outdir build\n',
    }, { ...ON, runbook: { ...ON.runbook, allowDocs: ['legacy/**'] } });
    expect(checkRunbook(r.ctx()).violations.map((v) => `${v.rule}:${v.file}:${v.line ?? ''}`).sort()).toEqual([
      'RUN-3:.github/workflows/bad.yml:4', 'RUN-3:.github/workflows/bad.yml:5', 'RUN-3:.github/workflows/bad.yml:7', 'RUN-4:docs/deploy-guide.md:',
    ]);
  });
});

describe('RUN-03 init runbook scaffolds without clobbering', () => {
  it('writes the template, stubs only missing scripts, enables the rules, and the result needs wiring', () => {
    const r = repo({ 'package.json': pkg({ deploy: 'wrangler deploy' }) }, BASE_CONFIG);
    expect(initRunbook({ cwd: r.dir }).written).toEqual(['RUNBOOK.md', 'package.json', 'guardrails.config.json']);
    const scripts = JSON.parse(readFileSync(join(r.dir, 'package.json'), 'utf8')).scripts;
    expect([scripts.deploy, scripts.smoke]).toEqual(['wrangler deploy', expect.stringMatching(/not wired yet.*exit 1$/)]);
    expect(checkRunbook(r.ctx()).violations).toEqual([]);
    expect(initRunbook({ cwd: r.dir }).written).toEqual([]);
  });
});

describe('RUN-04 smoke and secrets-check helpers', () => {
  it('smoke passes only when every path answers below 400; secrets-check never needs the values', async () => {
    const { createServer } = await import('node:http');
    const srv = createServer((req, res) => { res.statusCode = req.url === '/missing' ? 404 : 200; res.end('ok'); });
    await new Promise<void>((r) => srv.listen(0, '127.0.0.1', r));
    const url = `http://127.0.0.1:${(srv.address() as { port: number }).port}`;
    const { smoke, secretsCheck } = await import('../src/runbook-cmds.js');
    const good = await smoke(url, ['/', '/about']);
    const bad = await smoke(url, ['/', '/missing']);
    srv.close();
    expect([good.ok, bad.ok, bad.lines[1].slice(0, 8)]).toEqual([true, false, 'FAIL 404']);
    const list = `node -e "console.log('API_KEY: Value Encrypted\\nAPI_KEY_OLD: x')"`;
    expect(secretsCheck(['API_KEY', 'TOKEN'], list, process.cwd())).toEqual({ ok: false, missing: ['TOKEN'] });
    expect(secretsCheck(['API_KEY'], 'exit 1', process.cwd()).ok).toBe(false);
  });
});
