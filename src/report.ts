import type { Finding, Report } from './types.js';
import { ruleInfo } from './rules.js';

const loc = (f: Finding) => `${f.file}${f.line ? `:${f.line}` : ''}`;
const group = (fs: Finding[]) => {
  const m = new Map<string, Finding[]>();
  for (const f of fs) m.set(f.rule, [...(m.get(f.rule) ?? []), f]);
  return [...m.entries()].sort(([a], [b]) => a.localeCompare(b));
};

export function toText(r: Report, { verbose = false } = {}): string {
  const out: string[] = [];
  const live = r.findings.filter((f) => !f.known);
  for (const [rule, fs] of group(live)) {
    const info = ruleInfo(rule);
    out.push(`${fs[0].mode === 'block' ? '✖' : '⚠'} ${rule} ${info?.title ?? ''} (${fs.length})`);
    for (const f of fs.slice(0, verbose ? Infinity : 15)) out.push(`    ${loc(f)}  ${f.message}`);
    if (!verbose && fs.length > 15) out.push(`    … ${fs.length - 15} more (use --verbose)`);
    if (info) out.push(`    fix: ${info.fix}`);
  }
  if (r.counts.known) {
    out.push(`• ${r.counts.known} known (baselined) violations:`);
    for (const [rule, fs] of group(r.findings.filter((f) => f.known))) out.push(`    ${rule}: ${fs.length}`);
  }
  if (r.staleBaseline) out.push(`• ${r.staleBaseline} baseline entries are fixed. Run "guardrails baseline" to shrink it.`);
  for (const m of r.metrics) out.push(`• ${m.name}: ${m.value}${m.detail && !m.detail.includes('\n') ? ` (${m.detail})` : ''}`);
  out.push(`${r.ok ? 'PASS' : 'FAIL'}: ${r.counts.block} blocking, ${r.counts.warn} warnings, ${r.counts.known} known${r.base ? ` (diff vs ${r.base})` : ''}`);
  return out.join('\n');
}

export function toMarkdown(r: Report): string {
  const out: string[] = [];
  out.push(`### Guardrails: ${r.ok ? '✅ pass' : '❌ fail'}`);
  out.push(`${r.counts.block} blocking · ${r.counts.warn} warnings · ${r.counts.known} known (baselined)${r.base ? ` · diff vs \`${r.base}\`` : ''}`);
  const live = r.findings.filter((f) => !f.known);
  if (live.length) {
    out.push('', '| Rule | Mode | Where | What |', '|---|---|---|---|');
    for (const f of live.slice(0, 100))
      out.push(`| ${f.rule} | ${f.mode} | \`${loc(f)}\` | ${f.message.replace(/\|/g, '\\|').replace(/\n/g, ' ')} |`);
    if (live.length > 100) out.push(`| … | | | ${live.length - 100} more |`);
  }
  if (r.metrics.length) {
    out.push('', '| Metric | Value | Detail |', '|---|---|---|');
    for (const m of r.metrics) out.push(`| ${m.name} | ${m.value} | ${(m.detail ?? '').replace(/\n/g, '<br>').replace(/\|/g, '\\|')} |`);
  }
  if (r.staleBaseline) out.push('', `${r.staleBaseline} baseline entries are fixed; run \`guardrails baseline\` to shrink the baseline.`);
  return out.join('\n');
}

export function toJson(r: Report): string {
  return JSON.stringify(r, null, 2);
}
