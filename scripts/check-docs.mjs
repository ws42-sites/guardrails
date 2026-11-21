import { readFileSync } from 'node:fs';
import { AGENT_HOSTS } from '../dist/hook.js';
import { RULES } from '../dist/rules.js';

const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const changelog = readFileSync('CHANGELOG.md', 'utf8');
const released = new Set([...changelog.matchAll(/^## \[(\d+\.\d+\.\d+)\]/gm)].map((match) => match[1]));
const problems = [];
const count = RULES.length;
const hosts = `${AGENT_HOSTS.slice(0, -1).join(', ')} and ${AGENT_HOSTS.at(-1)}`;

if (!released.has(pkg.version)) problems.push(`CHANGELOG.md missing ## [${pkg.version}]`);

const readme = readFileSync('README.md', 'utf8');
if (!readme.includes(`There are ${count} rules`)) problems.push(`README.md must say "There are ${count} rules"`);
if (!readme.includes(hosts)) problems.push(`README.md is missing the host list "${hosts}"`);

const skill = readFileSync('skill/SKILL.md', 'utf8');
const skillCounts = [...skill.matchAll(/\b(\d+) rules\b/g)].map((match) => Number(match[1]));
if (!skillCounts.length || skillCounts.some((n) => n !== count)) problems.push(`skill/SKILL.md rule count must be ${count}`);

const docs = ['README.md', 'skill/SKILL.md', 'TESTING.md', 'RUNBOOK.md', 'AGENTS.md', 'template/AGENTS.md', 'template/TESTING.md', 'template/guardrails.md'];
for (const file of docs) {
  const text = readFileSync(file, 'utf8');
  for (const match of text.matchAll(/\bplanned\s+v?(\d+\.\d+\.\d+)\b/gi)) {
    if (released.has(match[1])) problems.push(`${file} still says planned ${match[1]}, which is released`);
  }
}

const workflow = readFileSync('template/site/workflow.yml', 'utf8');
const pin = workflow.match(/GUARDRAILS_VERSION:\s*'([^']+)'/);
if (pin?.[1] !== pkg.version) problems.push(`template/site/workflow.yml pins ${pin?.[1] ?? 'nothing'}; package.json is ${pkg.version}`);
const reusable = readFileSync('.github/workflows/guardrails.yml', 'utf8');
if (!reusable.includes(`@v${pkg.version}`)) problems.push(`.github/workflows/guardrails.yml example ref is not @v${pkg.version}`);

if (problems.length) {
  for (const problem of problems) console.error(problem);
  process.exit(1);
}
console.log(`docs ok: ${pkg.version}, ${count} rules, ${hosts}`);
