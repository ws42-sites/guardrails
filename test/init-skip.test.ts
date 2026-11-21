import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { init } from '../src/init.js';
import { run } from '../src/runner.js';
import { diffAgainst, listFiles } from '../src/util.js';
import { repo } from './helpers/repo.js';

const FRESH = ['guardrails.config.json', 'AGENTS.md', '.github/CODEOWNERS', 'vitest.config.ts', 'vitest.setup.ts'];
const inDeps = (file: string) => file.split('/').includes('node_modules');

describe('INIT-02 dependency directories stay out of a fresh check', () => {
  it('skips node_modules that git would otherwise list, and init fills in .gitignore', async () => {
    const r = repo({
      'package.json': '{"name":"x","version":"1.0.0"}\n',
      'src/main.ts': 'export const a = 1;\n',
      '.gitignore': 'dist/\n',
    });
    r.remove(...FRESH);
    r.commit('base');
    r.write({
      'node_modules/left-pad/index.test.ts': "it('pads', () => { expect(1).toBeTruthy() });\n",
      'node_modules/left-pad/README.md': '# left-pad\n',
      'pkg/node_modules/nested.test.ts': "it('nested', () => {});\n",
      'src/extra.ts': 'export const b = 2;\n',
    });
    const listed = listFiles(r.dir);
    expect([listed.some(inDeps), listed.includes('src/extra.ts')]).toEqual([false, true]);
    expect(Object.keys(diffAgainst(r.dir, 'main')).some(inDeps)).toBe(false);
    init({ cwd: r.dir, owner: '@me' });
    const ignore = readFileSync(join(r.dir, '.gitignore'), 'utf8');
    expect([ignore.startsWith('dist/\n'), ignore.includes('\nnode_modules/\n'), ignore.includes('\n.stryker-tmp/\n')]).toEqual([true, true, true]);
    expect(init({ cwd: r.dir, owner: '@me' }).written.includes('.gitignore')).toBe(false);
    const report = await run({ cwd: r.dir, knip: false });
    expect(report.findings.filter((f) => f.rule === 'TEST-9' || inDeps(f.file))).toEqual([]);
    expect(report.ok).toBe(true);
  });
});
