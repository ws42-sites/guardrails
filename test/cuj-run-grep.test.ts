import { expect, it, vi } from 'vitest';
import { runJourneys, scaffoldJourneys } from '../src/cuj-commands.js';
import { runCommand } from '../src/checks/run.js';
import { repo } from './helpers/repo.js';

vi.mock('../src/checks/run.js', () => ({ runCommand: vi.fn() }));

it('CUJ-10 grep selects exact journey IDs in full titles without matching file paths', () => {
  const item = { title: 'Visitor contacts business', priority: 'P0', steps: ['Open contact'], run: [{ goto: '/' }] };
  const r = repo({
    'node_modules/@playwright/test/package.json': '{"main":"index.js"}',
    'node_modules/@playwright/test/index.js': '',
  }, {
    version: 1,
    journeys: { resultsFile: 'results.json', items: { 'CUJ-CLIENT': item, 'CUJ-BAD': item } },
  });
  scaffoldJourneys(r.dir);
  vi.mocked(runCommand).mockImplementation((command) => {
    const match = command.match(/--grep "([^"]+)"/);
    expect(match).not.toBeNull();
    const grep = new RegExp(match![1]);
    expect(grep.test('CUJ-CLIENT.spec.ts CUJ-CLIENT A visitor requests a quote')).toBe(true);
    expect(grep.test('e2e/cuj/CUJ-CLIENT.spec.ts \u203A CUJ-CLIENT A visitor requests a quote')).toBe(true);
    expect(grep.test('CUJ-BAD Something')).toBe(true);
    expect(grep.test('CUJ-CLIENT2.spec.ts CUJ-CLIENT2 Other')).toBe(false);
    expect(grep.test('CUJ-CLIENT.spec.ts')).toBe(false);
    expect(grep.test('CUJ-CLIENTX.spec.ts')).toBe(false);
    expect(grep.test('CUJ-OTHER.spec.ts CUJ-OTHER mentions CUJ-CLIENT here')).toBe(false);
    expect(grep.test('unrelated.spec.ts An unrelated title')).toBe(false);
    expect(grep.test('e2e/cuj/CUJ-CLIENT folder/unrelated.spec.ts \u203A An unrelated title')).toBe(false);
    r.write({ 'results.json': JSON.stringify({ suites: [{ specs: [
      { title: 'CUJ-CLIENT contact', ok: true }, { title: 'CUJ-BAD Something', ok: true },
    ] }] }) });
    return { code: 0, stdout: '', stderr: '' };
  });
  expect(runJourneys(r.dir, r.ctx().config, {
    baseUrl: 'https://preview.test', ids: ['CUJ-CLIENT', 'CUJ-BAD'],
  }).code).toBe(0);
});
