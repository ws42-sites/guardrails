// Test-suite audit (SUITE-1..6), ported from nexus-core tools/scripts/check-tests.mjs and made generic.
// Findings are counted per file and compared with a committed baseline, so only regressions block.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import type { GuardrailsConfig } from './config.js';
import { lineCount, matchesAny } from './util.js';

export const TEST_FILE = /\.(test|spec)\.[cm]?[jt]sx?$/;
const CODE_FILE = /\.[cm]?[jt]sx?$/;
const GENERATED = /(\.d\.ts|\.generated\.[^/]+|\.min\.[^/]+)$/;
const SKIP_SEGMENTS = new Set(['node_modules', 'dist', 'build', 'out', 'coverage', 'vendor', 'fixtures', '__fixtures__', '.wrangler', '.astro', '.next', 'legacy', 'archive']);
const TEST_SEGMENTS = new Set(['test', 'tests', '__tests__', 'e2e']);
const WORKSPACES = new Set(['apps', 'packages', 'services', 'libs', 'infra', 'semantic-packages']);

const FOCUSED = /\b(test|it|describe|suite)\.only\s*\(/;
const SKIPPED = /\b(test|it|describe|suite)\.(skip|todo)\s*\(|\bx(it|test|describe)\s*\(|\b[a-z]\.(skip|todo)\s*\(|\{\s*(skip|todo)\s*:\s*(?!false\b)\S/;
/** A skip is explained by an issue reference, TEST-SKIP marker, comment or reason string on its line (or a comment just above), or a platform/env condition. */
const SKIP_EXPLAINED = /#\d{2,}\b|issues?\/\d+|TEST-SKIP:|\/\/|\/\*|\b(skip|todo)\s*:\s*["'`]|process\.(platform|env)/;
const ASSERTION = /\bassert(\.\w+)?\s*\(|\bexpect\s*\(/;
const CLOCK = /\bDate\.now\s*\(|\bnew Date\s*\(\s*\)|\bperformance\.now\s*\(|\bprocess\.hrtime/;
const FAKE_TIMERS = /mock\.timers|useFakeTimers|FakeTimers|\.timers\.enable\(|setSystemTime|page\.clock/;
const READ_FILE = /\breadFile(Sync)?\s*\(/;
const SLEEP = /\bsetTimeout\s*\(\s*\w+\s*,\s*(\d+)\s*\)|\b(sleep|delay|wait|pause|waitForTimeout)\s*\(\s*(\d+)\s*\)/;
const POLL = /\bdeadline\b|^\s*while\s*\(|^\s*for\s*\(/;
const LOOP = /^\s*(for|while)\s*\(|\.forEach\s*\(/;
const TEST_HEADER = /^(\s*)(?:await\s+)?(?:[a-z]\.)?(?:test|it)\s*\(\s*(?:(["'`])(.*?)\2|$)/;
const BODY_END = /^\s*\}\)?;?\s*$/;
const LAYER_SUFFIX = /-(routes?|flow|integration|postgres|store|runtime|contract|e2e|service|http|api|endpoints?|unit|hardening|recovery|persistence|lifecycle|parity|validation)$/;

/** Finding kinds; each maps to the rule that gates it. */
export const KINDS = {
  focused: { rule: 'SUITE-1', text: '.only silences every other test' },
  skip: { rule: 'SUITE-1', text: 'skip/todo with no reason, issue or TEST-SKIP marker' },
  'source-text': { rule: 'SUITE-1', text: 'reads a product source file as text to assert on it' },
  oversize: { rule: 'SUITE-2', text: 'test file over the line cap' },
  sleep: { rule: 'SUITE-3', text: 'fixed sleep instead of waiting for the event' },
  clock: { rule: 'SUITE-4', text: 'real clock read inside an assertion, no fake timers' },
  'copy-paste': { rule: 'SUITE-5', text: '3+ tests in one file share a body shape' },
  matrix: { rule: 'SUITE-5', text: 'two files share 3+ test titles (duplicate family)' },
} as const;
export type Kind = keyof typeof KINDS;

export interface SuiteFinding { kind: Kind; file: string; line: number; detail: string }
export interface SuiteFile { path: string; lines: number; titles: string[] }
export interface Totals { testFiles: number; testLines: number; productFiles: number; productLines: number; ratio: number }
export interface SuiteAudit {
  findings: SuiteFinding[];
  counts: Record<string, number>;
  oversize: Record<string, number>;
  totals: Totals;
  packages: (Totals & { package: string })[];
  biggest: { path: string; lines: number; tests: number }[];
  families: { package: string; stem: string; files: number; lines: number }[];
}
export interface SuiteBaseline { version: 1; maxLines: number; totals: Totals; byKind: Record<string, number>; counts: Record<string, number>; oversize: Record<string, number> }

const norm = (p: string) => p.split('\\').join('/');
const dirs = (p: string) => norm(p).split('/').slice(0, -1);

export function isSuiteTest(cfg: GuardrailsConfig, path: string): boolean {
  const p = norm(path);
  if (matchesAny(p, cfg.testSuite.exclude)) return false;
  if (!(TEST_FILE.test(p) || matchesAny(p, cfg.tests.globs) || matchesAny(p, cfg.journeys.e2eGlobs))) return false;
  return CODE_FILE.test(p) && !dirs(p).some((s) => SKIP_SEGMENTS.has(s) && s !== 'fixtures');
}

export function isProduct(cfg: GuardrailsConfig, path: string): boolean {
  const p = norm(path);
  if (!CODE_FILE.test(p) || GENERATED.test(p) || isSuiteTest(cfg, p) || matchesAny(p, cfg.testSuite.exclude)) return false;
  if (/(^|\/)(\.[^/]+|scripts|tools)\//.test(p) || /\.(config|setup)\.[cm]?[jt]s$/.test(p)) return false;
  return !dirs(p).some((s) => SKIP_SEGMENTS.has(s) || TEST_SEGMENTS.has(s));
}

export function packageOf(path: string): string {
  const parts = norm(path).split('/');
  if (WORKSPACES.has(parts[0]) && parts.length > 2) return `${parts[0]}/${parts[1]}`;
  return parts.length > 1 ? parts[0] : '(root)';
}

const title = (t: string) => t.toLowerCase().replace(/[^a-z0-9 ]+/g, ' ').replace(/\s+/g, ' ').trim();
const shape = (body: string[]) => body.map((l) => l.replace(/(["'`])(?:\\.|(?!\1).)*\1/g, '$1$1').replace(/\b\d+(\.\d+)?\b/g, '0').replace(/\s+/g, '')).filter(Boolean).join('\n');

function collectTests(lines: string[]): { title: string; line: number; body: string[] }[] {
  const tests = [];
  for (let i = 0; i < lines.length; i++) {
    const h = TEST_HEADER.exec(lines[i]);
    if (!h) continue;
    const indent = h[1].length;
    const t = h[3] ?? /^\s*(["'`])(.*?)\1/.exec(lines[i + 1] ?? '')?.[2] ?? '';
    let end = i + 1;
    while (end < lines.length && !(lines[end].trim() && lines[end].length - lines[end].trimStart().length <= indent && BODY_END.test(lines[end]))) end++;
    tests.push({ title: t, line: i + 1, body: lines.slice(i + 1, end) });
  }
  return tests;
}

const insideLoop = (lines: string[], i: number) => {
  const own = lines[i].length - lines[i].trimStart().length;
  for (let b = i - 1; b >= Math.max(0, i - 40); b--) {
    const l = lines[b];
    if (l.trim() && l.length - l.trimStart().length < own && LOOP.test(l)) return true;
  }
  return false;
};

/** Scan one test file. `sourceRe` matches statements that name product source paths. */
export function scanTest(path: string, source: string, maxLines: number, sourceRe: RegExp): { findings: SuiteFinding[]; lines: number; titles: string[] } {
  const lines = source.split(/\r?\n/);
  const findings: SuiteFinding[] = [];
  const add = (kind: Kind, line: number, detail: string) => findings.push({ kind, file: path, line, detail: detail.slice(0, 100) });
  const total = lineCount(source);
  if (total > maxLines) add('oversize', 1, `${total} lines (cap ${maxLines})`);
  const fakes = FAKE_TIMERS.test(source);
  lines.forEach((line, i) => {
    const code = line.trim();
    if (!code || code.startsWith('//') || code.startsWith('*')) return;
    if (FOCUSED.test(line)) add('focused', i + 1, code);
    if (SKIPPED.test(line) && !SKIP_EXPLAINED.test(line) && !/^\s*(\/\/|\/\*|\*)/.test(lines[i - 1] ?? '')) add('skip', i + 1, code);
    if (READ_FILE.test(line) && sourceRe.test(/[(,]\s*$/.test(line) ? lines.slice(i, i + 3).join(' ') : line)) add('source-text', i + 1, code);
    const s = SLEEP.exec(line);
    if (s && Number(s[1] ?? s[3]) >= 20 && !POLL.test(line) && !insideLoop(lines, i)) add('sleep', i + 1, code);
    if (!fakes && ASSERTION.test(line) && CLOCK.test(line)) add('clock', i + 1, code);
  });
  const tests = collectTests(lines);
  const shapes = new Map<string, typeof tests>();
  for (const t of tests) {
    if (t.body.length < 3) continue;
    const k = createHash('sha1').update(shape(t.body)).digest('hex');
    shapes.set(k, [...(shapes.get(k) ?? []), t]);
  }
  for (const g of shapes.values()) if (g.length >= 3) add('copy-paste', g[0].line, `${g.length} tests share one body: ${g.map((t) => `"${t.title}"`).join(', ')}`);
  return { findings, lines: total, titles: tests.map((t) => t.title) };
}

/** Pairs of files sharing >= 3 normalised test titles (and >= 25% of the smaller file's titles). */
export function duplicateFamilies(files: SuiteFile[]): { files: [string, string]; shared: number }[] {
  const sized = files.filter((f) => f.titles.length >= 3);
  const byTitle = new Map<string, string[]>();
  for (const f of sized) for (const t of new Set(f.titles.map(title))) if (t) byTitle.set(t, [...(byTitle.get(t) ?? []), f.path]);
  const shared = new Map<string, number>();
  for (const paths of byTitle.values()) {
    if (paths.length < 2 || paths.length > 12) continue;
    for (let a = 0; a < paths.length; a++) for (let b = a + 1; b < paths.length; b++) shared.set(`${paths[a]}\u0000${paths[b]}`, (shared.get(`${paths[a]}\u0000${paths[b]}`) ?? 0) + 1);
  }
  const sizes = new Map(sized.map((f) => [f.path, new Set(f.titles.map(title)).size]));
  const out: { files: [string, string]; shared: number }[] = [];
  for (const [k, n] of shared) {
    const [a, b] = k.split('\u0000');
    if (n >= 3 && n / Math.min(sizes.get(a) ?? 1, sizes.get(b) ?? 1) >= 0.25) out.push({ files: [a, b], shared: n });
  }
  return out.sort((x, y) => y.shared - x.shared || x.files[0].localeCompare(y.files[0]));
}

const totalsOf = (t: Omit<Totals, 'ratio'>): Totals => ({ ...t, ratio: t.productLines ? Number((t.testLines / t.productLines).toFixed(2)) : 0 });

export function auditSuite(cwd: string, cfg: GuardrailsConfig, files: string[]): SuiteAudit {
  const roots = [...new Set([...cfg.sourceRoots, 'src'])].map((r) => r.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  const sourceRe = new RegExp(`(^|["'\`/(,\\s])(\\.\\.\\/)*(${roots.join('|')})\\/[^"'\`]*\\.[cm]?[jt]sx?|["'\`]src["'\`]`);
  const pk = new Map<string, Omit<Totals, 'ratio'>>();
  const bucket = (p: string) => { const k = packageOf(p); if (!pk.has(k)) pk.set(k, { testFiles: 0, testLines: 0, productFiles: 0, productLines: 0 }); return pk.get(k)!; };
  const scanned: SuiteFile[] = [];
  const findings: SuiteFinding[] = [];
  const read = (f: string) => { try { const b = readFileSync(join(cwd, f)); return b.includes(0) ? null : b.toString('utf8'); } catch { return null; } };
  for (const f of files) {
    if (isSuiteTest(cfg, f)) {
      const src = read(f);
      if (src === null) continue;
      const r = scanTest(f, src, cfg.testSuite.maxLines, sourceRe);
      const b = bucket(f); b.testFiles++; b.testLines += r.lines;
      scanned.push({ path: f, lines: r.lines, titles: r.titles });
      findings.push(...r.findings);
    } else if (isProduct(cfg, f)) {
      const src = read(f);
      if (src === null) continue;
      const b = bucket(f); b.productFiles++; b.productLines += lineCount(src);
    }
  }
  for (const d of duplicateFamilies(scanned)) findings.push({ kind: 'matrix', file: d.files[0], line: 1, detail: `${d.shared} titles shared with ${d.files[1]}` });
  const counts: Record<string, number> = {};
  const oversize: Record<string, number> = {};
  for (const x of findings) {
    const k = x.kind === 'matrix' ? `matrix|${x.file}|${x.detail.split(' with ')[1]}` : `${x.kind}|${x.file}`;
    if (x.kind === 'oversize') oversize[x.file] = Number(x.detail.split(' ')[0]);
    else counts[k] = (counts[k] ?? 0) + 1;
  }
  const families = new Map<string, { package: string; stem: string; files: number; lines: number }>();
  for (const f of scanned) {
    const name = basename(f.path).replace(TEST_FILE, '');
    let stem = name;
    while (LAYER_SUFFIX.test(stem)) stem = stem.replace(LAYER_SUFFIX, '');
    if (stem === name || stem.length < 4) continue;
    const k = `${packageOf(f.path)}:${stem}`;
    const g = families.get(k) ?? { package: packageOf(f.path), stem, files: 0, lines: 0 };
    g.files++; g.lines += f.lines; families.set(k, g);
  }
  const sum = (key: keyof Omit<Totals, 'ratio'>) => [...pk.values()].reduce((a, t) => a + t[key], 0);
  return {
    findings: findings.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line),
    counts, oversize,
    totals: totalsOf({ testFiles: sum('testFiles'), testLines: sum('testLines'), productFiles: sum('productFiles'), productLines: sum('productLines') }),
    packages: [...pk].filter(([, t]) => t.testFiles || t.productFiles).map(([p, t]) => ({ package: p, ...totalsOf(t) })).sort((a, b) => b.testLines - a.testLines),
    biggest: [...scanned].sort((a, b) => b.lines - a.lines).slice(0, 10).map((f) => ({ path: f.path, lines: f.lines, tests: f.titles.length })),
    families: [...families.values()].filter((g) => g.files >= 3).sort((a, b) => b.lines - a.lines),
  };
}

export const byKind = (a: Pick<SuiteAudit, 'counts' | 'oversize'>): Record<string, number> => {
  const out: Record<string, number> = { oversize: Object.keys(a.oversize).length };
  for (const [k, n] of Object.entries(a.counts)) { const kind = k.split('|')[0]; out[kind] = (out[kind] ?? 0) + n; }
  return out;
};

export function toBaseline(a: SuiteAudit, maxLines: number): SuiteBaseline {
  const sort = (o: Record<string, number>) => Object.fromEntries(Object.entries(o).sort(([x], [y]) => x.localeCompare(y)));
  return { version: 1, maxLines, totals: a.totals, byKind: sort(byKind(a)), counts: sort(a.counts), oversize: sort(a.oversize) };
}

export function readSuiteBaseline(cwd: string, file: string): SuiteBaseline | null {
  const p = join(cwd, file);
  return existsSync(p) ? (JSON.parse(readFileSync(p, 'utf8')) as SuiteBaseline) : null;
}

export function writeSuiteBaseline(cwd: string, file: string, b: SuiteBaseline): void {
  mkdirSync(dirname(join(cwd, file)), { recursive: true });
  writeFileSync(join(cwd, file), `${JSON.stringify(b, null, 2)}\n`);
}

/** Ratchet: keep each entry at min(old, current) so fixes drop out and nothing new gets in. */
export function ratchetSuite(old: SuiteBaseline, cur: SuiteBaseline): SuiteBaseline {
  const min = (o: Record<string, number>, c: Record<string, number>) => Object.fromEntries(Object.entries(c).filter(([k]) => k in o).map(([k, v]) => [k, Math.min(v, o[k])]));
  const counts = min(old.counts, cur.counts);
  const oversize = min(old.oversize, cur.oversize);
  return { ...cur, counts, oversize, byKind: byKind({ counts, oversize }) };
}

export interface Regression { kind: Kind; file: string; message: string; key: string }

/** What got worse than the baseline: a count above its entry, or a file newly over (or further over) the cap. */
export function regressions(a: SuiteAudit, b: SuiteBaseline | null): Regression[] {
  const base: Pick<SuiteBaseline, 'counts' | 'oversize'> = b ?? { counts: {}, oversize: {} };
  const out: Regression[] = [];
  for (const [k, n] of Object.entries(a.counts)) {
    const was = base.counts[k] ?? 0;
    if (n <= was) continue;
    const [kind, file, other] = k.split('|') as [Kind, string, string?];
    const sample = a.findings.filter((f) => f.kind === kind && f.file === file).map((f) => `line ${f.line}: ${f.detail}`)[0] ?? '';
    out.push({ kind, file, key: k, message: other ? `shares 3+ test titles with ${other} (duplicate family). Merge them or make them test different behaviour.` : `${KINDS[kind].text}: ${n} (baseline ${was}). ${sample}` });
  }
  for (const [file, lines] of Object.entries(a.oversize)) {
    const was = base.oversize[file];
    if (was !== undefined && lines <= was) continue;
    out.push({ kind: 'oversize', file, key: `oversize|${file}`, message: was === undefined ? `${lines} lines, over the ${b?.maxLines ?? 300}-line cap. Split it by behaviour.` : `grew from ${was} to ${lines} lines (over the cap). Shrink it back.` });
  }
  return out;
}

export function suiteMarkdown(a: SuiteAudit, regs: Regression[], hasBaseline: boolean): string {
  const k = byKind(a);
  const out = ['## Test suite (guardrails check-tests)', '',
    `test files ${a.totals.testFiles}, test lines ${a.totals.testLines}, product lines ${a.totals.productLines}, **test/product ${a.totals.ratio}**`, '',
    '| finding | count | rule |', '|---|---|---|',
    ...Object.entries(KINDS).map(([kind, m]) => `| ${kind} (${m.text}) | ${k[kind] ?? 0} | ${m.rule} |`), '',
    hasBaseline ? (regs.length ? `**${regs.length} regression(s) vs the baseline:**` : 'No regressions vs the baseline.') : 'No baseline yet: run `guardrails check-tests --update-baseline` and commit it.',
    ...regs.map((r) => `- \`${KINDS[r.kind].rule}\` ${r.file}: ${r.message}`), '',
    '| package | test files | test lines | product lines | ratio |', '|---|---|---|---|---|',
    ...a.packages.slice(0, 12).map((p) => `| ${p.package} | ${p.testFiles} | ${p.testLines} | ${p.productLines} | ${p.productLines ? p.ratio : '-'} |`), '',
    'Biggest test files: ' + a.biggest.slice(0, 5).map((f) => `${f.path} (${f.lines})`).join(', '),
  ];
  if (a.families.length) out.push('', 'Layer families (same stem, 3+ files): ' + a.families.slice(0, 5).map((f) => `${f.package} ${f.stem} (${f.files} files, ${f.lines} lines)`).join(', '));
  return `${out.join('\n')}\n`;
}
