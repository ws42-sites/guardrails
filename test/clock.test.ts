import { execFileSync } from 'node:child_process';
import { symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { describe, expect, it } from 'vitest';
import { checkClock, CLOCK_SHIFT_MODULE } from '../src/checks/clock.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const SHIFTED_DAYS = `
const realNow = performance.timeOrigin + performance.now();
console.log(JSON.stringify({
  days: Math.round((Date.now() - realNow) / 864e5),
  fixed: new Date(0).getTime(),
  isDate: new Date() instanceof Date,
  str: typeof Date(),
}));`;

describe('CLOCK-01 the clock-shift preload moves "now" and nothing else', () => {
  it('shifts Date.now/new Date() by N years and leaves explicit dates alone', () => {
    const out = execFileSync(process.execPath, ['--import', pathToFileURL(CLOCK_SHIFT_MODULE).href, '-e', SHIFTED_DAYS], {
      env: { ...process.env, GUARDRAILS_CLOCK_SHIFT_YEARS: '1' }, encoding: 'utf8',
    });
    expect(JSON.parse(out)).toEqual({ days: 365, fixed: 0, isDate: true, str: 'string' });
  });

  it('reports a test that only fails with the clock shifted (TEST-6)', () => {
    const r = repo({
      'package.json': '{"type":"module"}',
      'src/bomb.test.ts': `import { it, expect } from 'vitest';\nit('CORE-01 bomb', () => { expect(Date.now() - performance.timeOrigin).toBeLessThan(365 * 864e5) });\nit('CORE-01 safe', () => { expect(1).toBe(1) });\n`,
    }, { ...BASE_CONFIG, tests: { command: 'npx vitest run --pool=forks --root .' } });
    symlinkSync(join(process.cwd(), 'node_modules'), join(r.dir, 'node_modules'));
    const out = checkClock(r.ctx());
    expect(out.violations.map((v) => [v.rule, v.file, v.message.split(' passes')[0]])).toEqual([['TEST-6', 'src/bomb.test.ts', '"CORE-01 bomb"']]);
  }, 60_000);
});
