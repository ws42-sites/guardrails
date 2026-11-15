// Regenerates the rules table in RULES.md from src/rules.ts (via dist). RULES-01 checks they agree.
import { readFileSync, writeFileSync } from 'node:fs';
import { RULES } from '../dist/rules.js';
const md = readFileSync('RULES.md', 'utf8');
const esc = (s) => s.replaceAll('|', '\\|');
const rows = RULES.map((r) => `| ${r.id} | ${r.mode}${r.diffOnly ? ' (diff)' : ''} | ${esc(r.title)} | ${esc(r.why)} | ${esc(r.fix)} |`).join('\n');
const start = md.indexOf('| ID |');
const end = md.indexOf('\n\n', start);
writeFileSync('RULES.md', `${md.slice(0, start)}| ID | Default | Rule | Why | Fix |\n|---|---|---|---|---|\n${rows}${md.slice(end)}`);
