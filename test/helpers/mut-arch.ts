import type { Finding, Mode, Violation } from '../../src/types.js';
import { BASE_CONFIG } from './repo.js';

export interface FindingRow {
  rule: string;
  file: string;
  line: number | null;
  message: string;
  fingerprint: string;
  mode: Mode;
}

export interface ViolationRow {
  rule: string;
  file: string;
  line: number | null;
  message: string;
  fingerprint: string;
}

export function sortFindings(findings: Finding[]): FindingRow[] {
  return findings
    .map((f) => ({ rule: f.rule, file: f.file, line: f.line ?? null, message: f.message, fingerprint: f.fingerprint, mode: f.mode }))
    .sort((a, b) => a.rule.localeCompare(b.rule) || a.file.localeCompare(b.file) || (a.line ?? 0) - (b.line ?? 0) || a.fingerprint.localeCompare(b.fingerprint));
}

export function sortViolations(vs: Violation[]): ViolationRow[] {
  return vs
    .map((v) => ({ rule: v.rule, file: v.file, line: v.line ?? null, message: v.message, fingerprint: v.fingerprint }))
    .sort((a, b) => a.rule.localeCompare(b.rule) || a.file.localeCompare(b.file) || (a.line ?? 0) - (b.line ?? 0) || a.fingerprint.localeCompare(b.fingerprint));
}

export const BOUNDARY_CONFIG = {
  ...BASE_CONFIG,
  sourceRoots: ['src', 'packages/web', 'sub/', 'mods/*.ts'],
  architecture: {
    forbidden: [
      {
        name: 'core-only',
        from: ['src/pages/**', 'src/views/**'],
        to: ['src/secret/key.ts', 'src/secret/token.ts'],
        except: ['src/pages/exempt/**'],
        message: 'Use facade.',
      },
      {
        from: ['src/pages/**'],
        to: ['src/raw/**'],
      },
      {
        name: 'no-legacy',
        from: ['src/views/**'],
        to: ['src/legacy/**'],
        message: 'Drop legacy.',
      },
    ],
    singletons: ['unusedFn', 'onceFn', 'dupFn', 'dupConst', 'dupAsync'],
    restrictedStrings: [{ pattern: 'raw://ban', allowIn: ['src/pages/exempt/**'], message: 'Use client.' }],
  },
  fixtures: { globs: ['**/fixtures/**'], publicDir: 'static/pub/', devOnly: ['src/dev/**', 'web/dev/**'] },
  tests: { globs: ['**/*.test.ts', 'test/**/*.html'], helperGlobs: ['src/helpers/**'] },
  journeys: { e2eGlobs: ['src/e2e/**'] },
} as const;

export const BOUNDARY_FILES: Record<string, string> = {
  'tsconfig.json': JSON.stringify({ compilerOptions: { baseUrl: '.', paths: { '@/*': ['src/*'] } } }),
  'src/secret/key.ts': 'export const key = 1;\n',
  'src/secret/token.ts': 'export const token = 2;\n',
  'src/raw/db.ts': 'export const db = 3;\n',
  'src/legacy/old.ts': 'export type Old = number;\n',
  'src/fixtures/fx.ts': 'export const fx = 5;\n',
  'src/fixtures/f2.ts': 'import { key } from "../secret/key";\nimport { fx } from "./fx";\nexport const f2 = [key, fx];\n',
  'src/fixtures/binary.jpg': 'x = "\\8"\n',
  'src/pages/exempt/ok.ts': 'import { key } from "../../secret/key";\nexport const ok = ["raw://ban", key];\n',
  'src/pages/a.ts': [
    'import { key } from "../secret/key"; // raw://ban',
    'import { db } from "../raw/db";',
    'import { fx } from "../fixtures/fx";',
    'export function onceFn() { return ["/logo.png", key, db, fx]; }',
    'export function dupFn() { return "/fixtures/trap.png"; }',
    'export const dupConst = "/fixtures/hero.png";\n',
  ].join('\n'),
  'src/views/b.ts': [
    'import { token } from "@/secret/token";',
    'import type { Old } from "../legacy/old";',
    'export function* dupFn() { yield token; }',
    'export const b: [Old, string] = [1, "raw://ban"];\n',
  ].join('\n'),
  'packages/web/app.ts': 'import { fx } from "../../src/fixtures/fx";\nexport let dupConst: number = fx;\n',
  'sub/entry.ts': 'import { fx } from "../src/fixtures/fx";\nexport const obj = { async dupAsync() { return fx; } };\n',
  'mods/plug.ts': 'import { fx } from "../src/fixtures/fx";\nexport var dupAsync = () => fx;\n',
  'src/pages/a.test.ts': 'import { key } from "../secret/key";\nimport { fx } from "../fixtures/fx";\nexport function onceFn() { return [key, fx, "raw://ban", "/fixtures/hero.png"]; }\nit("CORE-01 ok", () => { expect(1).toBe(1); });\n',
  'src/helpers/h.ts': 'import { key } from "../secret/key";\nimport { fx } from "../fixtures/fx";\nexport function onceFn() { return [key, fx, "raw://ban", "/fixtures/hero.png"]; }\n',
  'src/dev/seed.ts': 'import { key } from "../secret/key";\nimport { fx } from "../fixtures/fx";\nexport function onceFn() { return [key, fx, "raw://ban", "/fixtures/hero.png"]; }\n',
  'src/e2e/journey.ts': 'import { key } from "../secret/key";\nimport { fx } from "../fixtures/fx";\nexport const j = [key, fx];\n',
  'static/pub/fixtures/hero.png': 'png',
  'static/pub/logo.png': 'png',
  '0123456789/fixtures/trap.png': 'png',
  'web/landing.html': '<img src="/fixtures/hero.png">\n',
  'web/theme.scss': '.bg { background: url("/fixtures/hero.png"); }\n',
  'test/view.html': '<img src="/fixtures/hero.png">\n',
  'src/helpers/preview.html': '<img src="/fixtures/hero.png">\n',
  'web/dev/mock.html': '<img src="/fixtures/hero.png">\n',
  'static/pub/index.html': '<img src="/fixtures/hero.png">\n',
  'web/landing.html.bak': '<img src="/fixtures/hero.png">\n',
  'AGENTS.md': '# Agents /fixtures/hero.png raw://ban\n',
};
