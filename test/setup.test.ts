import { describe, expect, it } from 'vitest';
import { checkSetup } from '../src/checks/setup.js';
import { repo } from './helpers/repo.js';

const found = (r: ReturnType<typeof repo>) => checkSetup(r.ctx()).violations.map((v) => `${v.rule} ${v.file}: ${v.message.split('. ')[0]}`);
const withTest = { 'src/a.test.ts': "it('CORE-01 a', () => { expect(f()).toBe(1) });" };

describe('SETUP-01 every test must assert (TEST-9)', () => {
  it.each([
    ['the package setup', "export default { test: { setupFiles: ['@ws42/guardrails/vitest-setup'] } };", null, []],
    ['a local setup with hasAssertions', "export default { test: { setupFiles: './vitest.setup.ts' } };", undefined, []],
    ['a setup without hasAssertions', "export default { test: { setupFiles: ['./vitest.setup.ts'] } };", "beforeEach(() => {});", ['TEST-9 vitest.config.ts: no Vitest setup file calls expect.hasAssertions()']],
    ['no setupFiles', 'export default { test: {} };', null, ['TEST-9 vitest.config.ts: no Vitest setup file calls expect.hasAssertions()']],
  ])('%s', (_name, config, setup, expected) => {
    const r = repo({ ...withTest, 'vitest.config.ts': config });
    if (setup === null) r.remove('vitest.setup.ts');
    else if (setup) r.write({ 'vitest.setup.ts': setup });
    expect(found(r)).toEqual(expected);
  });
});

describe('SETUP-02 CODEOWNERS guards config, workflows and baseline (CFG-2)', () => {
  it.each([
    ['* @team\n', []],
    ['/guardrails.config.json @o\n/.github/workflows/ @o\n', ['CFG-2 .github/CODEOWNERS: does not assign an owner to .guardrails/baseline.json.']],
    ['/.github/ @o\n/.guardrails/ @o\nguardrails.config.json @o\nguardrails.config.json\n', ['CFG-2 .github/CODEOWNERS: does not assign an owner to guardrails.config.json.']],
  ])('%j', (codeowners, expected) => {
    expect(found(repo({ '.github/CODEOWNERS': codeowners }))).toEqual(expected);
  });

  it('requires a CODEOWNERS file', () => {
    expect(found(repo().remove('.github/CODEOWNERS'))).toEqual(['CFG-2 .github/CODEOWNERS: missing']);
  });
});
