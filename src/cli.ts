#!/usr/bin/env node
import { parseArgs } from 'node:util';
import { run } from './runner.js';
import { toJson, toMarkdown, toText } from './report.js';
import { init } from './init.js';
import { RULES, ruleInfo } from './rules.js';
import { loadConfig } from './config.js';
import { readBaseline, ratchet, writeBaseline } from './baseline.js';
import { collect, BASELINEABLE } from './runner.js';
import { specMatrix } from './checks/tests.js';
import { validateTest } from './validate.js';
import type { CheckContext } from './types.js';

const HELP = `guardrails <command> [options]

Commands
  init                 Scaffold guardrails.config.json, AGENTS.md (+ pointers), CODEOWNERS, CI workflow
  check                Run all checks. Exit 1 on blocking findings.
  report               Run checks and print a report (--format text|json|markdown)
  baseline             Write/ratchet the baseline (only shrinks; --force records everything current)
  specs                Print the spec matrix (ID, tier, tests)
  explain <RULE>       Explain a rule
  validate-test <ID>   Check a spec ID before writing a test

Options
  --base <ref>         Diff mode against <ref> (default: origin/$GITHUB_BASE_REF on PRs)
  --labels <a,b>       PR labels (default: $GUARDRAILS_LABELS)
  --clock-shift        Also run TEST-6 (suite with clock at +N years)
  --mutation           Also run TEST-8 (Stryker on mutation-tier files)
  --mutate <globs>     Restrict --mutation to matching files (comma list)
  --no-knip            Skip ARCH-3 reachability
  --only <groups>      Comma list: docs,tests,lint,arch,clock,mutation
  --format <f>         text (default) | json | markdown
  --cwd <dir>          Project root (default: current directory)
  --owner <@handle>    init: CODEOWNERS owner
  --force              init: overwrite files; baseline: record all current violations
  --verbose            Show every finding`;

async function main(): Promise<number> {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      base: { type: 'string' }, labels: { type: 'string' }, 'clock-shift': { type: 'boolean' }, mutation: { type: 'boolean' },
      'no-knip': { type: 'boolean' }, only: { type: 'string' }, format: { type: 'string' }, cwd: { type: 'string' },
      owner: { type: 'string' }, force: { type: 'boolean' }, verbose: { type: 'boolean' }, help: { type: 'boolean', short: 'h' },
      lines: { type: 'string' }, file: { type: 'string' }, mutate: { type: 'string' },
    },
  });
  const [cmd, arg] = positionals;
  const cwd = values.cwd ?? process.cwd();
  const base = values.base ?? (process.env.GITHUB_BASE_REF ? `origin/${process.env.GITHUB_BASE_REF}` : null);
  const labels = (values.labels ?? process.env.GUARDRAILS_LABELS ?? '').split(',').map((s) => s.trim()).filter(Boolean);
  const opts = { cwd, base, labels, clockShift: values['clock-shift'], mutation: values.mutation, knip: !values['no-knip'], only: values.only?.split(','), mutate: values.mutate?.split(',') };

  switch (cmd) {
    case 'init': {
      const r = init({ cwd, owner: values.owner, base: values.base, force: values.force });
      for (const f of r.written) console.log(`wrote   ${f}`);
      for (const f of r.skipped) console.log(`skipped ${f} (exists; --force to overwrite)`);
      console.log('Next: add specs to guardrails.config.json, run "guardrails baseline", then "guardrails check".');
      return 0;
    }
    case 'check':
    case 'report': {
      const r = await run(opts);
      const fmt = values.format ?? (cmd === 'report' ? 'text' : 'text');
      console.log(fmt === 'json' ? toJson(r) : fmt === 'markdown' ? toMarkdown(r) : toText(r, { verbose: values.verbose }));
      return cmd === 'check' && !r.ok ? 1 : 0;
    }
    case 'baseline': {
      const config = loadConfig(cwd);
      const { violations } = await collect({ ...opts, base: null, clockShift: false, mutation: false });
      const old = readBaseline(cwd, config.baselineFile);
      const first = old.entries.length === 0;
      const r = ratchet(old, violations, first || !!values.force, BASELINEABLE);
      writeBaseline(cwd, config.baselineFile, r.baseline);
      console.log(`${config.baselineFile}: ${r.baseline.entries.length} entries (${r.removed} removed, ${r.added} added)${first ? ' [initial]' : ''}`);
      if (r.added && !first) console.log('Baseline grew: CI will block this (BASE-1) unless a code owner adds the rebaseline label.');
      return 0;
    }
    case 'specs': {
      const config = loadConfig(cwd);
      const ctx: CheckContext = { cwd, config, base: null, labels: [], options: { clockShift: false, mutation: false, knip: false } };
      const rows = specMatrix(ctx);
      if (values.format === 'json') console.log(JSON.stringify(rows, null, 2));
      else for (const r of rows) console.log(`${r.id.padEnd(14)} ${r.tier}  ${String(r.tests.length).padStart(2)} tests  ${r.behaviour}`);
      return 0;
    }
    case 'explain': {
      const info = arg ? ruleInfo(arg.toUpperCase()) : undefined;
      if (!info) {
        for (const r of RULES) console.log(`${r.id.padEnd(8)} ${r.mode.padEnd(5)} ${r.title}`);
        return arg ? 1 : 0;
      }
      console.log(`${info.id}: ${info.title}\nDefault mode: ${info.mode}${info.diffOnly ? ' (diff mode only)' : ''}\nWhy: ${info.why}\nFix: ${info.fix}`);
      return 0;
    }
    case 'validate-test': {
      if (!arg) { console.error('usage: guardrails validate-test <SPEC-ID> [--file f] [--lines n]'); return 2; }
      const r = validateTest({ cwd, specId: arg, testFile: values.file, plannedLines: values.lines ? Number(values.lines) : undefined, base });
      console.log(JSON.stringify(r, null, 2));
      return r.ok ? 0 : 1;
    }
    default:
      console.log(HELP);
      return cmd && !values.help ? 2 : 0;
  }
}

main().then((code) => process.exit(code), (e) => { console.error(e instanceof Error ? e.message : e); process.exit(2); });
