#!/usr/bin/env node
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import { run } from './runner.js';
import { toJson, toMarkdown, toText } from './report.js';
import { RULES, ruleInfo } from './rules.js';
import { loadConfig } from './config.js';
import { specMatrix } from './checks/tests.js';
import { validateTest } from './validate.js';
import type { CheckContext } from './types.js';
import { addFlag } from './flags.js';
import { survivingMutants } from './checks/mutation.js';
import { journeyMatrix } from './checks/journeys.js';

const VERSION = '0.1.0';
const text = (t: string) => ({ content: [{ type: 'text' as const, text: t }] });
const fail = (e: unknown) => ({ ...text(`error: ${e instanceof Error ? e.message : String(e)}`), isError: true });
const cwdArg = z.string().optional().describe('Project root. Defaults to the server working directory (or GUARDRAILS_CWD).');
const rootOf = (cwd?: string) => cwd ?? process.env.GUARDRAILS_CWD ?? process.cwd();

export function createServer(): McpServer {
  const server = new McpServer({ name: 'guardrails', version: VERSION });

  server.registerTool('guardrails_check', {
    description: 'Run the guardrails checks (docs, tests, ESLint bans, architecture). Call before every commit. Returns PASS/FAIL with what to fix.',
    inputSchema: {
      cwd: cwdArg,
      base: z.string().optional().describe('Git ref for diff mode (e.g. origin/main). Enables budget, weakening and new-dependency checks.'),
      only: z.array(z.enum(['docs', 'setup', 'tests', 'lint', 'arch', 'gaming', 'flags', 'scope', 'journeys', 'failbase', 'clock', 'mutation'])).optional(),
      clockShift: z.boolean().optional().describe('Also rerun the test suite with the clock shifted (slow).'),
    },
  }, async ({ cwd, base, only, clockShift }) => {
    try {
      const r = await run({ cwd: rootOf(cwd), base: base ?? null, only, clockShift, knip: !only || only.includes('arch') });
      return text(toText(r));
    } catch (e) { return fail(e); }
  });

  server.registerTool('guardrails_report', {
    description: 'Full guardrails report as JSON or markdown (for a PR comment).',
    inputSchema: { cwd: cwdArg, base: z.string().optional(), format: z.enum(['json', 'markdown']).default('markdown') },
  }, async ({ cwd, base, format }) => {
    try {
      const r = await run({ cwd: rootOf(cwd), base: base ?? null });
      return text(format === 'json' ? toJson(r) : toMarkdown(r));
    } catch (e) { return fail(e); }
  });

  server.registerTool('guardrails_explain_rule', {
    description: 'Explain a guardrails rule ID (e.g. TEST-7): why it exists and how to fix a violation. Omit id to list all rules.',
    inputSchema: { id: z.string().optional() },
  }, async ({ id }) => {
    if (!id) return text(RULES.map((r) => `${r.id} (${r.mode}): ${r.title}`).join('\n'));
    const r = ruleInfo(id.toUpperCase());
    return r ? text(`${r.id}: ${r.title}\nmode: ${r.mode}${r.diffOnly ? ' (diff only)' : ''}\nwhy: ${r.why}\nfix: ${r.fix}`) : fail(`unknown rule ${id}`);
  });

  server.registerTool('guardrails_list_specs', {
    description: 'List spec IDs (behaviours) with tier and how many tests reference each. Use an existing ID for new tests.',
    inputSchema: { cwd: cwdArg, tier: z.enum(['T1', 'T2', 'T3']).optional() },
  }, async ({ cwd, tier }) => {
    try {
      const root = rootOf(cwd);
      const ctx: CheckContext = { cwd: root, config: loadConfig(root), base: null, labels: [], options: { clockShift: false, mutation: false, knip: false } };
      const rows = specMatrix(ctx).filter((r) => !tier || r.tier === tier);
      return text(JSON.stringify(rows.map((r) => ({ id: r.id, tier: r.tier, behaviour: r.behaviour, tests: r.tests.length })), null, 2));
    } catch (e) { return fail(e); }
  });

  server.registerTool('guardrails_validate_test', {
    description: 'Call BEFORE writing a test: checks the spec ID exists, the per-ID test cap, the test file path, and the test budget for the current diff.',
    inputSchema: {
      cwd: cwdArg,
      specId: z.string().describe('Spec ID the test will start with, e.g. REVIEW-03'),
      testFile: z.string().optional(),
      plannedLines: z.number().int().nonnegative().optional().describe('Approximate lines of test code you plan to add'),
      base: z.string().optional(),
    },
  }, async ({ cwd, specId, testFile, plannedLines, base }) => {
    try {
      return text(JSON.stringify(validateTest({ cwd: rootOf(cwd), specId, testFile, plannedLines, base: base ?? null }), null, 2));
    } catch (e) { return fail(e); }
  });

  server.registerTool('guardrails_flag', {
    description: 'STOP and flag: call this when a test conflicts with the spec, seems impossible, or would need a hack to pass. Records the conflict for a human (merge stays blocked until they resolve it). Never change code or tests just to satisfy a test.',
    inputSchema: {
      cwd: cwdArg,
      specId: z.string().describe('Spec ID the conflict is about, e.g. REVIEW-03'),
      file: z.string().describe('Test or source file involved'),
      reason: z.string().min(10).describe('What conflicts and why; what a human must decide'),
    },
  }, async ({ cwd, specId, file, reason }) => {
    try {
      const root = rootOf(cwd);
      const f = addFlag(root, loadConfig(root).flagsFile, { specId, file, reason, by: 'mcp' });
      return text(`Flagged ${f.id} (${specId}). Stop working on this spec and report the flag to the user. Merge is blocked (FLAG-1) until a human resolves it.`);
    } catch (e) { return fail(e); }
  });

  server.registerTool('guardrails_mutants', {
    description: 'Recompute surviving mutants (Stryker) for changed mutation-tier files: file, line, mutator, replacement. Use them to write assertions that kill real behaviour changes. The CI gate recomputes the score itself; never report a score.',
    inputSchema: { cwd: cwdArg, base: z.string().optional(), files: z.array(z.string()).optional().describe('Globs to restrict to') },
  }, async ({ cwd, base, files }) => {
    try {
      const root = rootOf(cwd);
      const ctx: CheckContext = { cwd: root, config: loadConfig(root), base: base ?? null, labels: [], options: { clockShift: false, mutation: true, knip: false, mutate: files } };
      return text(JSON.stringify(survivingMutants(ctx), null, 2));
    } catch (e) { return fail(e); }
  });

  server.registerTool('guardrails_list_journeys', {
    description: 'List Critical User Journeys (CUJs): ID, title, priority, steps, routes, specs and whether a human approved them. Read-only. Only approved journeys gate merges; agents cannot add or approve journeys (ask the owner).',
    inputSchema: { cwd: cwdArg, approvedOnly: z.boolean().optional() },
  }, async ({ cwd, approvedOnly }) => {
    try {
      const items = Object.entries(loadConfig(rootOf(cwd)).journeys.items).filter(([, j]) => !approvedOnly || j.approved);
      return text(JSON.stringify(items.map(([id, j]) => ({ id, ...j })), null, 2));
    } catch (e) { return fail(e); }
  });

  server.registerTool('guardrails_journey_coverage', {
    description: 'Journey coverage matrix: each journey with its specs (unit test counts), its e2e test(s) and status (draft, missing, covered, pass, fail). Read-only. An approved P0/P1 journey needs exactly one e2e test titled with its ID (JRN-1).',
    inputSchema: { cwd: cwdArg },
  }, async ({ cwd }) => {
    try {
      const root = rootOf(cwd);
      const ctx: CheckContext = { cwd: root, config: loadConfig(root), base: null, labels: [], options: { clockShift: false, mutation: false, knip: false } };
      return text(JSON.stringify(journeyMatrix(ctx), null, 2));
    } catch (e) { return fail(e); }
  });

  return server;
}

const isMain = process.argv[1] && /mcp\.(js|ts)$/.test(process.argv[1]);
if (isMain) {
  const server = createServer();
  await server.connect(new StdioServerTransport());
}
