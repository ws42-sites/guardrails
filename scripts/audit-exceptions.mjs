import { execFileSync } from 'node:child_process';

const accepted = new Map([
  ['GHSA-mrhx-c7xc-m3w5', 'Fixed only in Zod 4. The config parser and the published JSON schema stay on Zod 3 until that migration has its own release.'],
]);

function auditJson() {
  try {
    return execFileSync('npm', ['audit', '--json'], { encoding: 'utf8' });
  } catch (error) {
    const stdout = error.stdout?.toString?.() ?? '';
    if (!stdout) throw error;
    return stdout;
  }
}

const report = JSON.parse(auditJson());
const found = new Map();
for (const [name, info] of Object.entries(report.vulnerabilities ?? {})) {
  if (info.severity !== 'high' && info.severity !== 'critical') continue;
  const advisories = (info.via ?? []).filter((via) => via && typeof via === 'object' && via.url);
  if (!advisories.length) {
    found.set(`package:${name}`, `${info.severity} ${name} has no advisory URL`);
    continue;
  }
  for (const via of advisories) {
    const id = /GHSA-[\w-]+/.exec(via.url)?.[0] ?? via.url;
    found.set(id, `${info.severity} ${name}: ${via.title ?? id}`);
  }
}

const problems = [];
for (const [id, detail] of found) {
  if (!accepted.has(id)) problems.push(`${detail} (${id})`);
}
for (const [id, why] of accepted) {
  if (!found.has(id)) problems.push(`Stale audit exception ${id}. ${why}`);
}
if (problems.length) {
  for (const problem of problems) console.error(problem);
  console.error('Accepted high advisories:');
  for (const [id, why] of accepted) console.error(`  ${id}: ${why}`);
  process.exit(1);
}
const kept = [...accepted.keys()];
console.log(kept.length ? `audit ok; still accepted: ${kept.join(', ')}` : 'audit ok');
