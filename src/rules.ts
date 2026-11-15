import type { Mode } from './types.js';

export interface RuleInfo {
  id: string;
  title: string;
  mode: Mode;
  diffOnly?: boolean;
  why: string;
  fix: string;
}

/** The rules catalogue. RULES.md documents the same IDs; a test keeps them in sync. */
export const RULES: RuleInfo[] = [
  { id: 'CFG-1', title: 'Config is valid', mode: 'block', why: 'Unknown or misspelt keys would silently disable a rule.', fix: 'Fix guardrails.config.json so it matches schema/guardrails.schema.json.' },
  { id: 'ARCH-1', title: 'Declared import boundaries', mode: 'block', why: 'Agents copy whatever import path works; boundaries make the wrong one impossible.', fix: 'Import through the allowed layer (for example the storage interface), not across the forbidden edge.' },
  { id: 'ARCH-2', title: 'Single implementation per capability', mode: 'warn', why: 'Copies of the same helper drift; one copy ends up untested or unused.', fix: 'Delete the duplicate and import the one shared implementation.' },
  { id: 'ARCH-3', title: 'Production reachability', mode: 'warn', why: 'Code only tests exercise looks covered but never runs in production.', fix: 'Wire the module into a production entry, or delete it (and its tests).' },
  { id: 'ARCH-4', title: 'No unapproved new dependencies', mode: 'block', diffOnly: true, why: 'Adding a library is the usual workaround for a design problem.', fix: 'Remove the dependency. To keep it, a code owner adds it to deps.allow in guardrails.config.json (a CODEOWNER-protected change).' },
  { id: 'ARCH-5', title: 'Restricted strings', mode: 'block', why: 'Raw calls to a backend (for example api.github.com) bypass the layer that owns it.', fix: 'Move the call into the allowed module and call that module instead.' },
  { id: 'ARCH-6', title: 'No fixtures in production', mode: 'block', why: 'Demo data shipped to every user (a hard-coded fixture image) is a real bug.', fix: 'Only tests and dev-only code may reference fixtures. Load real data through the app.' },
  { id: 'TEST-1', title: 'Spec IDs resolve', mode: 'block', why: 'Every test must map to a promised behaviour; untagged or unknown tags are padding.', fix: 'Start the test (or its describe) title with a spec ID from the specs block, e.g. "REVIEW-03 expired token returns 410".' },
  { id: 'TEST-2', title: 'High-tier specs are tested', mode: 'block', why: 'Risky behaviours need at least one test; everything else is optional.', fix: 'Add one focused test for the spec ID, or remove the spec if it is no longer a behaviour.' },
  { id: 'TEST-3', title: 'Test budget', mode: 'warn', diffOnly: true, why: 'Test lines far above product lines usually means padding.', fix: 'Merge cases into it.each tables and drop tests that do not map to a distinct behaviour.' },
  { id: 'TEST-4', title: 'Test file size and tests per spec', mode: 'warn', why: 'Huge test files and many tests per spec are where duplication hides.', fix: 'Split by behaviour, use it.each, or delete redundant cases.' },
  { id: 'TEST-5', title: 'Banned test patterns', mode: 'block', why: 'Focused/skipped tests, snapshots in T1, real clocks, randomness, fake servers and Stryker-disable comments make tests lie.', fix: 'Remove .only/.skip/todo, use vi.useFakeTimers() + vi.setSystemTime(), fixed inputs instead of random loops, explicit assertions instead of snapshots.' },
  { id: 'TEST-5W', title: 'Weak assertions (heuristic)', mode: 'warn', why: 'Tests that only assert a mock was called, or only toBeDefined/toBeTruthy, rarely catch bugs.', fix: 'Assert on the observable result (returned value, written data, response status).' },
  { id: 'TEST-6', title: 'Clock independence', mode: 'block', why: 'Tests that depend on today\'s date break later (the Nov 30 time bomb).', fix: 'Freeze time with vi.useFakeTimers() and vi.setSystemTime(new Date("2026-01-01")) in the failing test.' },
  { id: 'TEST-7', title: 'No test weakening', mode: 'block', diffOnly: true, why: 'Deleting tests, removing expects, loosening matchers or lowering thresholds is the cheapest way to go green.', fix: 'Restore the test/assertion/threshold. Deleting base-branch tests needs the tests-removed-approved label from a code owner; tests added in the same PR can be deleted freely.' },
  { id: 'TEST-8', title: 'Mutation score', mode: 'block', why: 'The real answer to "do these tests test anything".', fix: 'Add assertions that would fail if the surviving mutants were real bugs.' },
  { id: 'DOC-1', title: 'Doc allowlist', mode: 'block', why: 'Agents create a new doc per task; they go stale and compete with the real instructions.', fix: 'Put the content in an allowlisted doc (or a check), then delete the file.' },
  { id: 'DOC-2', title: 'One agent instruction file', mode: 'block', why: 'AGENTS.md is the single source of truth for every coding tool.', fix: 'Make GEMINI.md/CLAUDE.md/etc. a one-line pointer to AGENTS.md.' },
  { id: 'DOC-3', title: 'Doc length cap', mode: 'warn', why: 'Long docs mean rules that should be checks.', fix: 'Move rules into checks and trim the doc.' },
  { id: 'BASE-1', title: 'Baseline only shrinks', mode: 'block', diffOnly: true, why: 'Re-baselining new violations would switch the checks off.', fix: 'Fix the violation instead. Growing the baseline needs the rebaseline label from a code owner.' },
];

export const RULE_IDS = RULES.map((r) => r.id);
export const ruleInfo = (id: string): RuleInfo | undefined => RULES.find((r) => r.id === id);
