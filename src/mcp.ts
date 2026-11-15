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
      only: z.array(z.enum(['docs', 'tests', 'lint', 'arch', 'clock', 'mutation'])).optional(),
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

  return server;
}

const isMain = process.argv[1] && /mcp\.(js|ts)$/.test(process.argv[1]);
if (isMain) {
  const server = createServer();
  await server.connect(new StdioServerTransport());
}
