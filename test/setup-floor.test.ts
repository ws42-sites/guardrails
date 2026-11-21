import { expect, it } from 'vitest';
import { checkSetup } from '../src/checks/setup.js';
import { repo } from './helpers/repo.js';

const details = (r: ReturnType<typeof repo>, files?: string[]) => checkSetup(r.ctx(), files).violations.map(({ rule, file, line, message }) => ({ rule, file, line, message }));
const testFile = { 'src/a.test.ts': "it('CORE-01 adds', () => { expect(add()).toBe(1) });" };
const missingSetup = (file: string) => ({ rule: 'TEST-9', file, line: undefined, message: 'no Vitest setup file calls expect.hasAssertions(). Add "@ws42ca/guardrails/vitest-setup" to test.setupFiles.' });

it.each([
  ['vite.config.mts', "setupFiles :['./support/assert']", 'support/assert.ts', 'expect.hasAssertions( \n )'],
  ['vitest.workspace.mjs', 'setupFiles: `./support/assert`', 'support/assert.js', 'expect.hasAssertions()'],
  ['vitest.config.cts', "setupFiles: ['missing', './support/assert.js']", 'support/assert.js', 'expect.hasAssertions(\t)'],
])('SETUP-01 resolves setup declarations in %s', (config, declaration, setup, body) => {
  const r = repo({ ...testFile, [config]: declaration, [setup]: body }).remove('vitest.config.ts', 'vitest.setup.ts');
  expect(details(r)).toEqual([]);
});

it.each(['prefixvitest.config.ts', 'vite.config.ts.bak', 'prefixvitest.workspace.js', 'vitest.workspace.js.bak'])('SETUP-01 rejects non-config filename %s', (file) => {
  const r = repo({ ...testFile, [file]: "setupFiles: ['@ws42ca/guardrails/vitest-setup']" }).remove('vitest.config.ts', 'vitest.setup.ts');
  expect(details(r)).toEqual([missingSetup('vitest.config.ts')]);
});

it.each([['vite.config.js', true], ['vitest.config.ts', false]])('SETUP-01 reports setup absence at %s', (file, present) => {
  const r = repo({ ...testFile }).remove('vitest.config.ts', 'vitest.setup.ts');
  if (present) r.write({ [file]: 'export default {}' });
  expect(details(r)).toEqual([missingSetup(file)]);
});

it('SETUP-01 needs no assertion setup without tests', () => {
  expect(details(repo().remove('vitest.config.ts', 'vitest.setup.ts'))).toEqual([]);
});

it.each([
  ['CODEOWNERS', '  # heading\r\n\r\n  *    @team   # owner\r\n', []],
  ['docs/CODEOWNERS', '/guardrails.config.json    @team\n/.github/workflows/ @team\n/.guardrails/ @team', []],
  ['CODEOWNERS', '* @team\n/guardrails.config.json # no owner\n', ['guardrails.config.json']],
  ['CODEOWNERS', 'nested/guardrails.config.json @team\n/.github/workflows/ @team\n/.guardrails/ @team', ['guardrails.config.json']],
  ['CODEOWNERS', '* @team\n/.github/workflows/guardrails.yml\n', ['.github/workflows/guardrails.yml']],
  ['CODEOWNERS', '* @team\n/.guardrails/baseline.json\n', ['.guardrails/baseline.json']],
])('SETUP-02 applies CODEOWNERS matching and precedence %s %j', (file, body, unowned) => {
  const r = repo({ [file]: body }).remove('.github/CODEOWNERS');
  expect(details(r)).toEqual(unowned.map((path) => ({ rule: 'CFG-2', file, line: undefined, message: `does not assign an owner to ${path}.` })));
});

it('SETUP-02 requires owners for both config formats', () => {
  const r = repo({ 'guardrails.md': '# config', '.github/CODEOWNERS': '/guardrails.config.json @team\n/.github/ @team\n/.guardrails/ @team' });
  expect(details(r)).toEqual([{ rule: 'CFG-2', file: '.github/CODEOWNERS', line: undefined, message: 'does not assign an owner to guardrails.md.' }]);
});

it('SETUP-02 explains all protected paths when CODEOWNERS is missing', () => {
  expect(details(repo().remove('.github/CODEOWNERS'))).toEqual([{ rule: 'CFG-2', file: '.github/CODEOWNERS', line: undefined, message: 'missing. It must assign an owner to guardrails.config.json, .github/workflows/guardrails.yml, .guardrails/baseline.json.' }]);
});

it('SETUP-02 discovers an owner file omitted from the supplied file list', () => {
  expect(details(repo(), [])).toEqual([]);
});
