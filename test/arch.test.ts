import { describe, expect, it } from 'vitest';
import { checkArch } from '../src/checks/arch.js';
import { BASE_CONFIG, repo } from './helpers/repo.js';

const cfg = {
  ...BASE_CONFIG,
  architecture: {
    forbidden: [{ name: 'via-index', from: ['src/**'], to: ['src/store/impl.ts'], except: ['src/store/index.ts'], message: 'Use src/store.' }],
    singletons: ['getToken'],
    restrictedStrings: [{ pattern: 'api\\.github\\.com', allowIn: ['src/store/impl.ts'], message: 'Go through the store.' }],
  },
};

describe('ARCH-01 import boundaries come from config (dependency-cruiser)', () => {
  it('flags forbidden imports (incl. tsconfig path aliases), honours except, ignores tests and non-source fixtures, and flags prod fixture imports', async () => {
    const r = repo({
      'src/store/impl.ts': 'export const impl = () => fetch("https://api.github.com");\n',
      'src/store/index.ts': 'export { impl } from "./impl";\n',
      'src/pages/a.ts': 'import { impl } from "../store/impl";\nimport { data } from "../fixtures/data";\nexport const a = [impl, data];\n',
      'src/pages/a.test.ts': 'import { impl } from "../store/impl";\nit("CORE-01", () => { expect(impl).toBeTypeOf("function") });\n',
      'src/fixtures/data.ts': 'export const data = 1;\n',
      'src/fixtures/hero.jpg': 'x = "\\8" // binary, not JS',
      'src/pages/b.ts': 'import { impl } from "@/store/impl";\nexport const b = impl;\n',
      'tsconfig.json': JSON.stringify({ compilerOptions: { baseUrl: '.', paths: { '@/*': ['src/*'] } } }),
    }, cfg);
    const v = (await checkArch(r.ctx())).violations;
    expect(v.map((x) => [x.rule, x.file, x.message]).sort()).toEqual([
      ['ARCH-1', 'src/pages/a.ts', 'imports src/store/impl.ts, crossing boundary "via-index". Use src/store.'],
      ['ARCH-1', 'src/pages/b.ts', 'imports src/store/impl.ts, crossing boundary "via-index". Use src/store.'],
      ['ARCH-6', 'src/pages/a.ts', 'imports fixture src/fixtures/data.ts. Only tests and dev-only code may load fixtures.'],
    ]);
  });
});

describe('ARCH-02 singletons, restricted strings and shipped fixture assets', () => {
  it('flags duplicate helpers, raw backend calls outside allowIn, and public fixtures referenced by prod code', async () => {
    const r = repo({
      'src/a.ts': 'export async function getToken() { return fetch("https://api.github.com/app"); }\n',
      'src/b.ts': 'export const getToken = async () => 1;\n',
      'src/c.ts': 'export const hero = "/fixtures/hero.jpg";\n',
      'public/fixtures/hero.jpg': 'jpg',
      'src/c.test.ts': 'it("CORE-01", () => { expect("/fixtures/hero.jpg").toContain("hero") });\n',
    }, cfg);
    const v = (await checkArch(r.ctx())).violations;
    expect(v.map((x) => [x.rule, x.file, x.line ?? null])).toEqual([
      ['ARCH-2', 'src/b.ts', null],
      ['ARCH-5', 'src/a.ts', 1],
      ['ARCH-6', 'src/c.ts', null],
    ]);
  });
});
