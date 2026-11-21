import { spawnSync } from 'node:child_process';

const run = spawnSync(process.execPath, ['dist/cli.js', 'check', '--only', 'docs', '--format', 'json'], { encoding: 'utf8' });
let report;
try {
  report = JSON.parse(run.stdout);
} catch {
  console.error(run.stderr || run.stdout || 'guardrails check --only docs did not print JSON');
  process.exit(1);
}
const doc1 = (report.findings ?? []).filter((finding) => finding.rule === 'DOC-1');
if (doc1.length) {
  for (const finding of doc1) console.error(`DOC-1 ${finding.file}: ${finding.message}`);
  process.exit(1);
}
if (run.status !== 0) {
  console.error(run.stderr || 'guardrails check --only docs failed');
  process.exit(run.status ?? 1);
}
console.log('DOC-1 ok');
