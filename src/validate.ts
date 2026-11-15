import { loadConfig } from './config.js';
import { indexTests, specMatrix } from './checks/tests.js';
import type { CheckContext } from './types.js';
import { diffAgainst, isProductFile, isTestFile, tierOf } from './util.js';

export interface ValidateTestInput { cwd: string; specId: string; testFile?: string; plannedLines?: number; base?: string | null }
export interface ValidateTestResult { ok: boolean; problems: string[]; warnings: string[]; spec?: { id: string; tier: string; behaviour: string; existingTests: number; maxTests: number }; budget?: string }

/** What an agent calls before writing a test: does the spec exist, is there room, is the budget OK. */
export function validateTest(input: ValidateTestInput): ValidateTestResult {
  const config = loadConfig(input.cwd);
  const ctx: CheckContext = { cwd: input.cwd, config, base: input.base ?? null, labels: [], options: { clockShift: false, mutation: false, knip: false } };
  const problems: string[] = [];
  const warnings: string[] = [];
  if (!new RegExp(config.specs.idPattern).test(input.specId)) problems.push(`"${input.specId}" does not match ${config.specs.idPattern}.`);
  const row = specMatrix(ctx, indexTests(ctx)).find((r) => r.id === input.specId);
  if (!row) {
    problems.push(`${input.specId} is not in guardrails.config.json specs.items. Ask a code owner to add the behaviour first, or reuse an existing ID.`);
    return { ok: false, problems, warnings };
  }
  if (row.tests.length >= config.specs.maxTestsPerId)
    problems.push(`${row.id} already has ${row.tests.length} tests (cap ${config.specs.maxTestsPerId}). Extend an it.each table instead of adding a test.`);
  if (input.testFile && !isTestFile(config, input.testFile)) problems.push(`${input.testFile} does not match tests.globs ${JSON.stringify(config.tests.globs)}.`);
  let budget: string | undefined;
  if (ctx.base) {
    const diff = diffAgainst(ctx.cwd, ctx.base);
    let prod = 0;
    let test = 0;
    for (const [f, d] of Object.entries(diff)) {
      const n = d.added.filter((l) => l.trim()).length;
      if (isTestFile(config, f)) test += n;
      else if (isProductFile(config, f) && tierOf(config, f) === row.tier) prod += n;
    }
    const after = test + (input.plannedLines ?? 0);
    const limit = config.tests.budget[row.tier];
    budget = prod ? `${after} test lines vs ${prod} ${row.tier} product lines = ${(after / prod).toFixed(2)}x (budget ${limit})` : `${after} test lines, no ${row.tier} product lines changed`;
    if (prod && after / prod > limit) warnings.push(`Over the ${row.tier} test budget: ${budget}. Prefer tightening existing tests.`);
  }
  return {
    ok: problems.length === 0,
    problems,
    warnings,
    spec: { id: row.id, tier: row.tier, behaviour: row.behaviour, existingTests: row.tests.length, maxTests: config.specs.maxTestsPerId },
    budget,
  };
}
