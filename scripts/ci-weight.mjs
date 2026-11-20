import { execFileSync } from 'node:child_process';

const [pr, main, nightly = main] = process.argv.slice(2);
if (!pr || !main) {
  console.error('usage: node scripts/ci-weight.mjs <prRunId> <mainRunId> [nightlyRunId]');
  process.exit(2);
}
const repo = process.env.GITHUB_REPOSITORY ?? 'ws42-sites/guardrails';
const jobs = (id) => JSON.parse(execFileSync('gh', ['api', `repos/${repo}/actions/runs/${id}/jobs?per_page=100`], { encoding: 'utf8' })).jobs
  .filter((j) => j.conclusion !== 'skipped' && j.started_at && j.completed_at)
  .map((j) => ({ name: j.name, min: (Date.parse(j.completed_at) - Date.parse(j.started_at)) / 60000 }));
const total = (js) => js.reduce((s, j) => s + j.min, 0);
const longest = (js) => Math.max(0, ...js.map((j) => j.min));
const p = jobs(pr);
const m = jobs(main);
const n = jobs(nightly);
const part = (v, cap) => 25 * Math.min(1, v / cap);
const P = longest(p);
const J = total(p);
const M = total(m);
const N = total(n);
const weight = part(P, 10) + part(J, 30) + part(M, 120) + part(N, 120);
const r = (v) => Math.round(v * 10) / 10;
console.log(JSON.stringify({ P: r(P), J: r(J), M: r(M), N: r(N), weight: r(weight), jobs: { pr: p, main: m, nightly: n } }, null, 2));
