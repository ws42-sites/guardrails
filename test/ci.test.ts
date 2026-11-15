import { describe, expect, it } from 'vitest';
import { checkCi } from '../src/checks/ci.js';
import { affectedTests } from '../src/affected.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const ON = { ...BASE_CONFIG, ci: { enabled: true } };
const msgs = (r: ReturnType<typeof repo>) => checkCi(r.ctx()).violations.map((v) => `${v.file}:${v.line ?? ''}:${v.message.split(/[.(]/)[0]}`);

describe('CI-01 PRs run affected tests; full suite nightly and before deploy', () => {
  it('warns on unconditional full PR runs and missing safety nets, and passes the standard shape', () => {
    const bad = repo({
      'package.json': JSON.stringify({ scripts: { deploy: 'wrangler deploy', 'deploy:preview': 'x', 'predeploy:preview': 'npm run test:changed' } }),
      '.github/workflows/ci.yml': 'on:\n  pull_request:\njobs:\n  t:\n    steps:\n      - run: npm test\n      - if: github.event_name == \'schedule\'\n        run: npx vitest run\n',
    }, ON);
    expect(msgs(bad)).toEqual([
      '.github/workflows/ci.yml:6:runs the full suite on every PR ',
      '.github/workflows::no scheduled ',
      'package.json::"deploy" has no full-suite safety net',
      'package.json::"deploy:preview" has no full-suite safety net',
    ]);
    const good = repo({
      'package.json': JSON.stringify({ scripts: { deploy: 'x', predeploy: 'npm run test:full', 'deploy:preview': 'x', 'predeploy:preview': 'npm run test:full' } }),
      '.github/workflows/ci.yml': 'on:\n  pull_request:\njobs:\n  t:\n    steps:\n      - run: npx vitest run --changed origin/main\n',
      '.github/workflows/nightly.yml': 'on:\n  schedule:\n    - cron: "0 7 * * *"\njobs:\n  t:\n    steps:\n      - run: npm run test:full\n',
    }, ON);
    expect([msgs(good), checkCi(repo({}).ctx()).violations]).toEqual([[], []]);
  });
});

describe('CI-02 guardrails affected selects tests through the import graph', () => {
  it('selects only tests that import or name a changed file; global files select all', async () => {
    const r = repo({ 'src/core/a.ts': 'export const a = 1;\n', 'src/lib/b.ts': 'export const b = 2;\n', 'src/core/a.test.ts': "import { a } from './a';\n", 'src/lib/b.test.ts': "import { b } from './b';\n", 'src/lib/page.test.ts': "readFileSync('public/about.html');\n", 'public/about.html': '<p>a</p>\n', 'package.json': '{}' }).commit();
    r.write({ 'src/core/a.ts': 'export const a = 3;\n', 'public/about.html': '<p>b</p>\n' });
    const one = await affectedTests(r.dir, r.ctx().config, 'HEAD');
    r.write({ 'package.json': '{"x":1}' });
    const all = await affectedTests(r.dir, r.ctx().config, 'HEAD');
    expect([one.all, one.tests, all.all, all.reason]).toEqual([false, ['src/core/a.test.ts', 'src/lib/page.test.ts'], true, 'package.json changed']);
  });
});
