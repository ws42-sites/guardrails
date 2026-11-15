import { existsSync } from 'node:fs';
import { join } from 'node:path';
import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { lineCount, listFiles, matchesAny, readText, violation } from '../util.js';

export const SECTIONS = ['Deploy', 'Rollback', 'Smoke', 'Secrets rotation', 'Incident first steps'] as const;
export const STANDARD_SCRIPTS = ['deploy', 'deploy:preview', 'rollback', 'smoke', 'secrets:check'] as const;
const SECTION_SCRIPT: Record<string, string | undefined> = { Deploy: 'deploy', Rollback: 'rollback', Smoke: 'smoke', 'Secrets rotation': 'secrets:check' };

const NPM_RUN = /\bnpm run ([\w:.-]+)/g;
const CLICK_PATH = /\b(click|tap)\b|\bnavigate to\b|\b(go to|open|log ?in to|sign in to)\b[^.`]*\b(dashboard|console|portal|web ui)\b/i;
/** Raw deploy commands that belong behind `npm run deploy` (RUN-3). */
const INLINE_DEPLOY = /\b(wrangler(\.js)?(\s+pages)?\s+deploy|cdk\s+deploy|vercel\s+(deploy|--prod)|netlify\s+deploy|fly(ctl)?\s+deploy|serverless\s+deploy|sls\s+deploy|sam\s+deploy|firebase\s+deploy|aws\s+s3\s+sync|render\s+deploy)\b|uses:\s*cloudflare\/(wrangler|pages)-action|api\.render\.com\/deploy/;
const SCRIPT_DEPLOY = /\bnpm run (deploy|deploy:preview)(?![\w:-])/;
const SCRIPT_SMOKE = /\bnpm run smoke(?![\w:-])/;
const RUNBOOKISH = /(^|\/)[^/]*(runbook|deploy|rollback|playbook)[^/]*\.mdx?$/i;

/** Split markdown into "## " sections, ignoring headings inside code fences. Code lines are marked. */
function sections(md: string): { title: string; line: number; prose: string[]; all: string }[] {
  const out: { title: string; line: number; prose: string[]; all: string }[] = [];
  let fence = false;
  md.split('\n').forEach((l, i) => {
    if (/^\s*```/.test(l)) fence = !fence;
    const h = !fence && /^##\s+(.+?)\s*#*\s*$/.exec(l);
    if (h) out.push({ title: h[1], line: i + 1, prose: [], all: '' });
    else if (out.length) {
      const s = out[out.length - 1];
      s.all += `${l}\n`;
      if (!fence && !/^\s*```/.test(l)) s.prose.push(l.replace(/`[^`]*`/g, ''));
    }
  });
  return out;
}

function scripts(ctx: CheckContext): Record<string, string> | null {
  const p = join(ctx.cwd, ctx.config.runbook.packageJson);
  if (!existsSync(p)) return null;
  try { return (JSON.parse(readText(ctx.cwd, ctx.config.runbook.packageJson)).scripts ?? {}) as Record<string, string>; } catch { return null; }
}

/** RUN-1 sections, RUN-2 scripts, RUN-3 workflows, RUN-4 length and stray runbooks. */
export function checkRunbook(ctx: CheckContext, files = listFiles(ctx.cwd)): CheckOutput {
  const rb = ctx.config.runbook;
  if (!rb.enabled) return { violations: [] };
  const v: Violation[] = [];
  const pkg = rb.packageJson;
  const have = scripts(ctx);
  const isNa = (s: string) => s in rb.na;

  // RUN-2: the standard scripts exist (or are n/a with a reason).
  for (const s of STANDARD_SCRIPTS)
    if (!isNa(s) && !(have && s in have)) v.push(violation('RUN-2', pkg, `missing script "${s}". Wire it to the real command or add runbook.na["${s}"] with the reason.`, `script:${s}`));

  for (const f of files) if (RUNBOOKISH.test(f) && f !== rb.file && !matchesAny(f, rb.allowDocs) && !matchesAny(f, ['node_modules/**', '**/node_modules/**']))
    v.push(violation('RUN-4', f, `runbook/deploy doc outside ${rb.file}. Fold it into ${rb.file} and delete it.`, 'stray'));

  // RUN-3: deploy workflows call the script and smoke afterwards.
  for (const f of files.filter((x) => matchesAny(x, rb.workflows))) {
    const lines = readText(ctx.cwd, f).split('\n');
    let deployAt = -1;
    let smokeAfter = false;
    lines.forEach((l, i) => {
      const code = l.replace(/(^|\s)#.*$/, '');
      if (/--dry-run\b/.test(code)) return; // builds and validates, deploys nothing
      if (INLINE_DEPLOY.test(code)) v.push(violation('RUN-3', f, `inline deploy command "${code.trim().slice(0, 80)}". Call \`npm run deploy\` (or deploy:preview) instead.`, `inline:${code.trim()}`, i + 1));
      if (SCRIPT_DEPLOY.test(code) || INLINE_DEPLOY.test(code)) { deployAt = i; smokeAfter = false; }
      else if (deployAt >= 0 && SCRIPT_SMOKE.test(code)) smokeAfter = true;
    });
    if (deployAt >= 0 && !smokeAfter) v.push(violation('RUN-3', f, 'deploys without running `npm run smoke` afterwards.', 'smoke', deployAt + 1));
  }

  if (!existsSync(join(ctx.cwd, rb.file))) {
    v.push(violation('RUN-1', rb.file, `missing ${rb.file}. Run "guardrails init runbook".`, 'missing'));
    return { violations: v };
  }
  const md = readText(ctx.cwd, rb.file);
  const n = lineCount(md);
  if (n > rb.maxLines) v.push(violation('RUN-4', rb.file, `${n} lines (cap ${rb.maxLines}). Keep commands, cut prose.`, 'cap'));

  // RUN-1: fixed sections in order, each pointing at a command, no click-paths.
  const found = sections(md);
  const titles = found.map((s) => s.title.toLowerCase());
  const idx = SECTIONS.map((t) => titles.indexOf(t.toLowerCase()));
  const missing = SECTIONS.filter((_, i) => idx[i] < 0);
  if (missing.length) v.push(violation('RUN-1', rb.file, `missing section(s): ${missing.join(', ')}. Sections: ${SECTIONS.join(', ')}.`, 'sections'));
  else if (idx.some((x, i) => i > 0 && x < idx[i - 1])) v.push(violation('RUN-1', rb.file, `sections out of order. Use: ${SECTIONS.join(', ')}.`, 'order'));
  for (const t of SECTIONS) {
    const s = found.find((x) => x.title.toLowerCase() === t.toLowerCase());
    if (!s) continue;
    const script = SECTION_SCRIPT[t];
    const cmds = [...s.all.matchAll(NPM_RUN)].map((m) => m[1]);
    if (!cmds.length && !(script && isNa(script) && /\bn\/a\b/i.test(s.all)))
      v.push(violation('RUN-1', rb.file, `"${t}" must point to a command (\`npm run ${script ?? 'smoke'}\`)${script ? ', or say n/a when runbook.na lists it' : ''}.`, `cmd:${t}`, s.line));
    s.prose.forEach((l, i) => {
      if (CLICK_PATH.test(l)) v.push(violation('RUN-1', rb.file, `"${t}" has a manual click-path ("${l.trim().slice(0, 60)}"). Script it and point to the command.`, `click:${t}:${l.trim()}`, s.line + 1 + i));
    });
  }

  // RUN-2: every referenced command exists.
  for (const m of md.matchAll(NPM_RUN)) {
    const s = m[1];
    if (!(have && s in have)) v.push(violation('RUN-2', rb.file, `references \`npm run ${s}\` but ${pkg} has no "${s}" script.`, `ref:${s}`));
  }
  return { violations: v };
}
