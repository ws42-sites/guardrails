import { expect, it } from 'vitest';
import { checkSetup } from '../src/checks/setup.js';
import { repo } from './helpers/repo.js';

it('SETUP-01 accepts current and legacy setup packages and recommends the current package when missing', () => {
  const r = repo({
    'src/a.test.ts': "it('CORE-01 a', () => { expect(f()).toBe(1) });",
    'vitest.config.ts': "export default { test: { setupFiles: ['@ws42ca/guardrails/vitest-setup'] } };",
  }).remove('vitest.setup.ts');
  expect(checkSetup(r.ctx()).violations.filter((v) => v.rule === 'TEST-9')).toEqual([]);

  r.write({ 'vitest.config.ts': "export default { test: { setupFiles: ['@ws42/guardrails/vitest-setup'] } };" });
  expect(checkSetup(r.ctx()).violations.filter((v) => v.rule === 'TEST-9')).toEqual([]);

  r.write({ 'vitest.config.ts': 'export default { test: {} };' });
  const violations = checkSetup(r.ctx()).violations.filter((v) => v.rule === 'TEST-9');
  expect(violations).toHaveLength(1);
  expect(violations[0].message).toContain('@ws42ca/guardrails/vitest-setup');
});
