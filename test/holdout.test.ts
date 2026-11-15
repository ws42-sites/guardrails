import { mkdirSync, mkdtempSync, symlinkSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { runHoldout } from '../src/holdout.js';
import { checkTests } from '../src/checks/tests.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const VITEST = `node ${JSON.stringify(join(process.cwd(), 'node_modules/vitest/vitest.mjs'))} run`;
const config = { ...BASE_CONFIG, tests: { command: VITEST }, holdout: { repo: 'ws42-sites/x-holdout', specs: ['CORE-01'] } };

function hidden(body: string): string {
  const dir = mkdtempSync(join(tmpdir(), 'guardrails-hidden-'));
  mkdirSync(join(dir, 'core'));
  writeFileSync(join(dir, 'core/add.test.ts'), `import { expect, it } from 'vitest';\nimport { add } from '../../../src/core/add';\nit('CORE-01 hidden', () => { ${body} });\n`);
  return dir;
}

describe('HOLD-01 hidden holdout tests run only at the gate and report titles only', () => {
  it('passes when the hidden tests pass, blocks with the title (not the source) when they fail, and cleans up', () => {
    const r = repo({ '.gitignore': 'node_modules\n', 'src/core/add.ts': 'export const add = (a: number, b: number) => a + b;\n' }, config);
    symlinkSync(join(process.cwd(), 'node_modules'), join(r.dir, 'node_modules'));
    const cfg = r.ctx().config;
    expect(runHoldout(r.dir, cfg, hidden('expect(add(2, 2)).toBe(4)'))).toEqual({ violations: [], ran: 1 });
    const bad = runHoldout(r.dir, cfg, hidden('expect(add(2, 2)).toBe(5)'));
    expect([bad.ran, bad.violations.map((v) => v.message.split('.')[0])]).toEqual([1, ['hidden test failed: "CORE-01 hidden"']]);
    expect(existsSync(join(r.dir, 'test/holdout'))).toBe(false);
  });

  it('exempts holdout specs from TEST-2 and treats adding one as weakening', () => {
    const r = repo({}, { ...BASE_CONFIG, holdout: { specs: [] } }).commit();
    expect(checkTests(r.ctx()).violations.map((v) => v.rule)).toEqual(['TEST-2', 'TEST-2']);
    r.write({ 'guardrails.config.json': JSON.stringify(config) });
    const out = checkTests(r.ctx('main')).violations;
    expect(out.map((v) => `${v.rule} ${v.message.split('. ')[0]}`)).toEqual([
      'TEST-2 LIB-01 (T2) "lib formats text" has no test.',
      'TEST-7 config loosened: holdout.specs: added "CORE-01"',
    ]);
  });
});
