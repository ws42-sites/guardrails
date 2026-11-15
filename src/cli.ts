#!/usr/bin/env node
import { parseArgs } from 'node:util';
import { run } from './runner.js';
import { toJson, toMarkdown, toText } from './report.js';
import { init, initRunbook } from './init.js';
import { secretsCheck, smoke } from './runbook-cmds.js';
import { affectedTests } from './affected.js';
import { runCommand } from './checks/run.js';
import { RULES, ruleInfo } from './rules.js';
import { loadConfig } from './config.js';
import { readBaseline, ratchet, writeBaseline } from './baseline.js';
import { collect, baselineRules } from './runner.js';
import { specMatrix } from './checks/tests.js';
import { validateTest } from './validate.js';
import type { CheckContext } from './types.js';
import { addFlag } from './flags.js';
import { survivingMutants } from './checks/mutation.js';
import { guard, hookResponse, type Agent } from './hook.js';
import { runHoldout } from './holdout.js';
import { addDrafts, approveJourney, assertHuman, draftJourneys, presetDrafts } from './journeys.js';
import { journeyMatrix } from './checks/journeys.js';
import { listFiles } from './util.js';

const HELP = `guardrails <command> [options]

Commands
  init                 Scaffold guardrails.config.json, AGENTS.md (+ pointers), CODEOWNERS, CI workflow
  smoke --url <u> [paths]   Read-only GET of each path (default /); fails on a status >= 400. SMOKE_URL overrides --url
  secrets-check --require A,B -- <list cmd>   Fails unless every name appears in the provider's list output (never printed)
  affected [--base ref] [--e2e] [--run "<cmd>"]   Tests affected by the change (dependency graph); --run appends them to <cmd>, all of them when a global file changed
  init runbook         Scaffold RUNBOOK.md, stub deploy/deploy:preview/rollback/smoke/secrets:check scripts, enable RUN-1..4
  check                Run all checks. Exit 1 on blocking findings.
  report               Run checks and print a report (--format text|json|markdown)
  baseline             Write/ratchet the baseline (only shrinks; --force records everything current)
  specs                Print the spec matrix (ID, tier, tests)
  explain <RULE>       Explain a rule
  validate-test <ID>   Check a spec ID before writing a test
  flag <ID> <reason>   Stop and flag a spec/test conflict for a human (--file f). Blocks merge (FLAG-1)
  mutants              Recompute surviving mutants for changed files (--base, --mutate)
  hook-guard           Agent hook: deny edits to locked files (--agent claude|gemini|antigravity; JSON on stdin)
  holdout --dir <d>    Run hidden holdout tests from a private checkout (HOLD-1)
  journeys list        Journey coverage matrix (journey -> specs -> tests, e2e)
  journeys draft       Draft 5-10 unapproved journeys from docs, routes and mocks (LLM if GEMINI_API_KEY/OPENAI_API_KEY; --dry-run)
  journeys approve <ID> Approve a journey (humans only; --by <name>)

Options
  --base <ref>         Diff mode against <ref> (default: origin/$GITHUB_BASE_REF on PRs)
  --labels <a,b>       PR labels (default: $GUARDRAILS_LABELS)
  --clock-shift        Also run TEST-6 (suite with clock at +N years)
  --mutation           Also run TEST-8 (Stryker on mutation-tier files)
  --mutate <globs>     Restrict --mutation to matching files (comma list)
  --fails-on-base      Also run TEST-11 (new/changed tests must fail on base); needs --base
  --specs <IDs>        Spec IDs this task implements (SCOPE-1; default $GUARDRAILS_SPECS)
  --no-knip            Skip ARCH-3 reachability
  --only <groups>      Comma list: docs,runbook,ci,setup,tests,lint,arch,gaming,flags,scope,journeys,failbase,clock,mutation
  --format <f>         text (default) | json | markdown
  --cwd <dir>          Project root (default: current directory)
  --owner <@handle>    init: CODEOWNERS owner
  --preset <name>      init: add starter journeys as drafts (small-business)
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
      'fails-on-base': { type: 'boolean' }, specs: { type: 'string' }, agent: { type: 'string' }, dir: { type: 'string' },
      preset: { type: 'string' }, by: { type: 'string' }, 'dry-run': { type: 'boolean' }, url: { type: 'string' }, require: { type: 'string' }, e2e: { type: 'boolean' }, run: { type: 'string' },
    },
  });
  const [cmd, arg, ...rest] = positionals;
  const cwd = values.cwd ?? process.cwd();
  const base = values.base ?? (process.env.GITHUB_BASE_REF ? `origin/${process.env.GITHUB_BASE_REF}` : null);
  const labels = (values.labels ?? process.env.GUARDRAILS_LABELS ?? '').split(',').map((s) => s.trim()).filter(Boolean);
  const opts = { cwd, base, labels, clockShift: values['clock-shift'], mutation: values.mutation, knip: !values['no-knip'], only: values.only?.split(','), mutate: values.mutate?.split(','),
    failsOnBase: values['fails-on-base'], specs: (values.specs ?? process.env.GUARDRAILS_SPECS)?.split(',').map((x) => x.trim()).filter(Boolean) };

  switch (cmd) {
    case 'affected': {
      const cfg = loadConfig(cwd);
      const a = await affectedTests(cwd, cfg, base ?? `origin/${cfg.baseBranch}`);
      const list = values.e2e ? a.e2e : a.tests;
      console.error(a.all ? `affected: all (${a.reason})` : `affected: ${list.length} of ${a.changed.length} changed file(s) -> ${list.length} test file(s)`);
      if (!values.run) { for (const f of list) console.log(f); return 0; }
      if (!list.length) { console.error('affected: nothing to run'); return 0; }
      const r = runCommand(`${values.run} ${list.map((f) => JSON.stringify(f)).join(' ')}`, cwd, {}, 60 * 60_000);
      process.stdout.write(r.stdout); process.stderr.write(r.stderr);
      return r.code;
    }
    case 'smoke': {
      const url = process.env.SMOKE_URL || values.url;
      if (!url) { console.error('smoke: pass --url or set SMOKE_URL'); return 2; }
      const r = await smoke(url, positionals.slice(1));
      for (const l of r.lines) console.log(l);
      return r.ok ? 0 : 1;
    }
    case 'secrets-check': {
      const names = (values.require ?? '').split(',').map((x) => x.trim()).filter(Boolean);
      const sep = process.argv.indexOf('--');
      const cmdLine = sep >= 0 ? process.argv.slice(sep + 1).join(' ') : '';
      if (!names.length || !cmdLine) { console.error('secrets-check: --require A,B -- <provider list command>'); return 2; }
      const r = secretsCheck(names, cmdLine, cwd);
      console.log(r.ok ? `ok: ${names.length} required secret names present` : `missing: ${r.missing.join(', ')}`);
      return r.ok ? 0 : 1;
    }
    case 'init': {
      if (arg === 'runbook') {
        const rb = initRunbook({ cwd, force: values.force });
        for (const f of rb.written) console.log(`wrote   ${f}`);
        for (const f of rb.skipped) console.log(`skipped ${f}`);
        console.log('Next: wire each stub script to the real command (or add runbook.na with a reason), then "guardrails check --only runbook".');
        return 0;
      }
      const r = init({ cwd, owner: values.owner, base: values.base, force: values.force });
      for (const f of r.written) console.log(`wrote   ${f}`);
      if (values.preset) {
        assertHuman(process.env, 'guardrails init --preset');
        const added = addDrafts(cwd, presetDrafts(values.preset, listFiles(cwd)));
        console.log(`journeys: added ${added.length} unapproved drafts from ${values.preset} (${added.join(', ')}). Edit, link specs, then "guardrails journeys approve <ID>".`);
      }
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
      const r = ratchet(old, violations, first || !!values.force, baselineRules(config.rules));
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
    case 'flag': {
      const reason = rest.join(' ');
      if (!arg || !reason) { console.error('usage: guardrails flag <SPEC-ID> <reason> [--file path]'); return 2; }
      const config = loadConfig(cwd);
      const f = addFlag(cwd, config.flagsFile, { specId: arg, file: values.file ?? '', reason, by: process.env.GUARDRAILS_AGENT });
      console.log(`Flagged ${f.id} on ${f.specId} in ${config.flagsFile}. STOP work on this spec; a human must resolve it (merge is blocked by FLAG-1).`);
      return 0;
    }
    case 'mutants': {
      const config = loadConfig(cwd);
      const ctx: CheckContext = { cwd, config, base, labels, options: { clockShift: false, mutation: true, knip: false, mutate: opts.mutate } };
      console.log(JSON.stringify(survivingMutants(ctx), null, 2));
      return 0;
    }
    case 'hook-guard': {
      const agent = (values.agent ?? 'claude') as Agent;
      let input = '';
      for await (const chunk of process.stdin) input += chunk;
      let payload = {};
      try { payload = JSON.parse(input || '{}'); } catch { /* not JSON: allow */ }
      const r = hookResponse(agent, guard(payload));
      if (r.stdout) process.stdout.write(r.stdout);
      if (r.stderr) process.stderr.write(`${r.stderr}\n`);
      return r.code;
    }
    case 'journeys': {
      const config = loadConfig(cwd);
      if (arg === 'list' || !arg) {
        const ctx: CheckContext = { cwd, config, base: null, labels: [], options: { clockShift: false, mutation: false, knip: false } };
        const rows = journeyMatrix(ctx);
        if (values.format === 'json') console.log(JSON.stringify(rows, null, 2));
        else for (const j of rows) console.log(`${j.id.padEnd(16)} ${j.priority} ${j.status.padEnd(8)} ${j.title}\n${''.padEnd(26)}specs: ${j.specs.map((x) => `${x.id}(${x.tests})`).join(' ') || '-'}  e2e: ${j.e2e.join(', ') || '-'}`);
        return 0;
      }
      if (arg === 'draft') {
        const r = await draftJourneys(cwd);
        if (values['dry-run']) { console.log(JSON.stringify({ via: r.via, drafts: r.drafts }, null, 2)); return 0; }
        assertHuman(process.env, 'guardrails journeys draft (writing)');
        const added = addDrafts(cwd, r.drafts);
        console.log(`Drafted ${added.length} journeys via ${r.via} (approved: false): ${added.join(', ') || 'none new'}. Review them in guardrails.config.json, then "guardrails journeys approve <ID>".`);
        return 0;
      }
      if (arg === 'approve') {
        const id = rest[0];
        if (!id) { console.error('usage: guardrails journeys approve <CUJ-ID> [--by name]'); return 2; }
        const by = approveJourney(cwd, id, { by: values.by });
        console.log(`Approved ${id} (approvedBy: ${by}). It now gates merges (JRN-1).`);
        return 0;
      }
      console.error('usage: guardrails journeys list|draft|approve <ID>');
      return 2;
    }
    case 'holdout': {
      if (!values.dir) { console.error('usage: guardrails holdout --dir <holdout checkout>'); return 2; }
      const r = runHoldout(cwd, loadConfig(cwd), values.dir);
      for (const x of r.violations) console.log(`BLOCK ${x.rule} ${x.message}`);
      console.log(`holdout: ${r.ran} hidden tests ran, ${r.violations.length} problems`);
      return r.violations.length ? 1 : 0;
    }
    default:
      console.log(HELP);
      return cmd && !values.help ? 2 : 0;
  }
}

main().then((code) => process.exit(code), (e) => { console.error(e instanceof Error ? e.message : e); process.exit(2); });
