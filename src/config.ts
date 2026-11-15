import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { z } from 'zod';
import { RULE_IDS } from './rules.js';

const globs = z.array(z.string().min(1));
const tier = z.enum(['T1', 'T2', 'T3']);
const mode = z.enum(['block', 'warn', 'off']);

export const configSchema = z
  .object({
    $schema: z.string().optional(),
    version: z.literal(1),
    baseBranch: z.string().default('main'),
    sourceRoots: globs.default(['src']),
    tiers: z
      .object({ T1: globs.default([]), T2: globs.default([]), T3: globs.default([]) })
      .strict()
      .default({}),
    specs: z
      .object({
        idPattern: z.string().default('^[A-Z][A-Z0-9]{1,11}-[0-9]{2,4}$'),
        maxTestsPerId: z.number().int().positive().default(8),
        items: z
          .record(
            z
              .object({
                tier,
                behaviour: z.string().min(3),
                /** Tests for this spec may pass on base (they guard existing behaviour). TEST-11. */
                regressionGuard: z.boolean().optional(),
                /** Property-based tests (seeded fast-check with a time limit) allowed. TEST-5. */
                properties: z.boolean().optional(),
                /** Product paths this behaviour lives in. SCOPE-1. */
                paths: globs.optional(),
                /** Journeys this spec serves (back-reference; must match journeys.items.*.specs). JRN-2. */
                journeys: z.array(z.string()).optional(),
              })
              .strict(),
          )
          .default({}),
      })
      .strict()
      .default({}),
    architecture: z
      .object({
        tsConfig: z.string().optional(),
        entries: globs.default([]),
        forbidden: z
          .array(
            z
              .object({
                name: z.string().optional(),
                from: globs,
                to: globs,
                except: globs.optional().describe('Importers exempt from this rule'),
                message: z.string().optional(),
              })
              .strict(),
          )
          .default([]),
        singletons: z.array(z.string()).default([]),
        /** Files allowed to call process.exit (ARCH-7). */
        cliEntries: globs.default(['**/cli.{ts,js,mjs,cjs}', 'bin/**', 'scripts/**']),
        /** Product files allowed to read the test environment (ARCH-7), e.g. a test-only bootstrap. */
        testEnvAllow: globs.default([]),
        restrictedStrings: z
          .array(
            z
              .object({ pattern: z.string(), allowIn: globs.default([]), message: z.string() })
              .strict(),
          )
          .default([]),
      })
      .strict()
      .default({}),
    deps: z
      .object({
        allow: z.array(z.string()).default([]),
        registry: z
          .object({
            enabled: z.boolean().default(true),
            minAgeDays: z.number().nonnegative().default(30),
            minWeeklyDownloads: z.number().nonnegative().default(1000),
            cacheHours: z.number().positive().default(24),
          })
          .strict()
          .default({}),
      })
      .strict()
      .default({}),
    duplication: z
      .object({
        enabled: z.boolean().default(true),
        minLines: z.number().int().positive().default(5),
        minTokens: z.number().int().positive().default(50),
        maxPercent: z.number().min(0).max(100).default(3),
      })
      .strict()
      .default({}),
    scope: z
      .object({ maxProductLines: z.number().int().positive().default(600) })
      .strict()
      .default({}),
    holdout: z
      .object({
        repo: z.string().optional(),
        path: z.string().optional(),
        specs: z.array(z.string()).default([]),
        target: z.string().default('test/holdout'),
      })
      .strict()
      .optional(),
    lock: z
      .object({
        specTests: z.boolean().default(true),
        paths: globs.default(['guardrails.config.json', '.guardrails/**', '.github/CODEOWNERS', '.github/workflows/**', '.claude/settings.json', '.gemini/settings.json', '.agents/hooks.json']),
      })
      .strict()
      .default({}),
    journeys: z
      .object({
        idPattern: z.string().default('^CUJ-[A-Z0-9]{2,16}$'),
        /** Playwright (or other e2e) test files; excluded from unit-test rules. JRN-1. */
        e2eGlobs: globs.default(['e2e/**/*.spec.ts', 'e2e/**/*.test.ts']),
        /** Optional Playwright JSON reporter output, for pass/fail in the coverage matrix. */
        resultsFile: z.string().optional(),
        /** Files whose routes count as new routes/endpoints (JRN-4). */
        routeGlobs: globs.default(['src/pages/**', 'app/**/page.{ts,tsx,js,jsx}', 'pages/**', 'functions/**']),
        draft: z
          .object({ geminiModel: z.string().default('gemini-2.5-flash'), openaiModel: z.string().default('gpt-4.1-mini'), max: z.number().int().min(1).max(20).default(8) })
          .strict()
          .default({}),
        /** JRN-5 visual diff of per-step screenshots against mocks (VIS-1 v2). Stub; off by default. */
        visual: z
          .object({ enabled: z.boolean().default(false), mocksDir: z.string().default('design/mocks'), maxDiffRatio: z.number().min(0).max(1).default(0.01) })
          .strict()
          .default({}),
        items: z
          .record(
            z
              .object({
                title: z.string().min(10),
                priority: z.enum(['P0', 'P1', 'P2']),
                steps: z.array(z.string().min(3)).min(1),
                specs: z.array(z.string()).default([]),
                routes: z.array(z.string()).default([]),
                approved: z.boolean().default(false),
                approvedBy: z.string().optional(),
              })
              .strict(),
          )
          .default({}),
      })
      .strict()
      .default({}),
    fixtures: z
      .object({
        globs: globs.default(['**/fixtures/**', '**/__fixtures__/**']),
        publicDir: z.string().default('public'),
        devOnly: globs.default([]),
      })
      .strict()
      .default({}),
    tests: z
      .object({
        globs: globs.default(['**/*.test.ts', '**/*.test.tsx', '**/*.spec.ts', '**/*.spec.tsx']),
        helperGlobs: globs.default(['**/test/**', '**/__tests__/helpers/**']),
        budget: z
          .object({
            T1: z.number().positive().default(1),
            T2: z.number().positive().default(0.6),
            T3: z.number().positive().default(0.4),
          })
          .strict()
          .default({}),
        maxFileLines: z.number().int().positive().default(300),
        command: z.string().default('npx vitest run --pool=forks'),
        clockShiftYears: z.number().positive().default(2),
        testsRemovedLabel: z.string().default('tests-removed-approved'),
        propertyTimeLimitMs: z.number().int().positive().default(5000),
      })
      .strict()
      .default({}),
    mutation: z
      .object({
        tiers: z.array(tier).default(['T1']),
        break: z.number().min(0).max(100).default(70),
        timeoutMinutes: z.number().positive().default(15),
        excludedMutations: z.array(z.string()).default([]),
        /** Files covered by slow integration suites that the Stryker run excludes (GUARDRAILS_MUTATION=1). */
        exclude: globs.default([]),
      })
      .strict()
      .default({}),
    docs: z
      .object({
        allow: globs.default(['README.md', 'AGENTS.md', 'docs/ARCHITECTURE.md']),
        extensions: z.array(z.string()).default(['.md', '.mdx', '.txt', '.rst']),
        disguise: globs.default(['**/NOTES*', '**/TODO*', '**/*-plan.*', '**/*_plan.*']),
        ignore: globs.default(['public/**', 'node_modules/**', 'dist/**', '**/LICENSE*', '**/CHANGELOG.md']),
        maxLines: z.record(z.number().int().positive()).default({ default: 250, 'AGENTS.md': 150 }),
        agentFile: z.string().default('AGENTS.md'),
        pointerFiles: z
          .array(z.string())
          .default(['GEMINI.md', 'CLAUDE.md', '.cursorrules', '.windsurfrules', '.github/copilot-instructions.md']),
        requiredLinks: z.record(z.array(z.string())).default({}),
      })
      .strict()
      .default({}),
    rules: z
      .record(mode)
      .default({})
      .superRefine((rules, ctx) => {
        for (const id of Object.keys(rules))
          if (!RULE_IDS.includes(id)) ctx.addIssue({ code: 'custom', message: `unknown rule id ${id}` });
      }),
    baselineFile: z.string().default('.guardrails/baseline.json'),
    flagsFile: z.string().default('.guardrails/flags.json'),
    labels: z
      .object({
        rebaseline: z.string().default('guardrails:rebaseline'),
        characterization: z.string().default('allow-characterization'),
        flagsResolved: z.string().default('flags-resolved'),
        /** A code owner approved loosening guardrails.config.json in this PR (TEST-7 config part). */
        configApproved: z.string().default('guardrails:config-approved'),
      })
      .strict()
      .default({}),
  })
  .strict()
  .superRefine((cfg, ctx) => {
    const idRe = new RegExp(cfg.journeys.idPattern);
    for (const [id, j] of Object.entries(cfg.journeys.items)) {
      if (!idRe.test(id)) ctx.addIssue({ code: 'custom', path: ['journeys', 'items', id], message: `journey ID must match ${cfg.journeys.idPattern}` });
      for (const s of j.specs)
        if (!cfg.specs.items[s]) ctx.addIssue({ code: 'custom', path: ['journeys', 'items', id, 'specs'], message: `unknown spec ${s}` });
      if (j.approved && !j.approvedBy) ctx.addIssue({ code: 'custom', path: ['journeys', 'items', id, 'approvedBy'], message: 'approved journeys need approvedBy (use guardrails journeys approve)' });
    }
    for (const [sid, s] of Object.entries(cfg.specs.items))
      for (const jid of s.journeys ?? []) {
        const j = cfg.journeys.items[jid];
        if (!j) ctx.addIssue({ code: 'custom', path: ['specs', 'items', sid, 'journeys'], message: `unknown journey ${jid}` });
        else if (!j.specs.includes(sid)) ctx.addIssue({ code: 'custom', path: ['specs', 'items', sid, 'journeys'], message: `${jid} does not list ${sid} in its specs` });
      }
  });

export type GuardrailsConfig = z.output<typeof configSchema>;
export const CONFIG_FILE = 'guardrails.config.json';

export class ConfigError extends Error {}

export function parseConfig(raw: unknown): GuardrailsConfig {
  const result = configSchema.safeParse(raw);
  if (!result.success) {
    const lines = result.error.issues.map((i) => `${i.path.join('.') || '(root)'}: ${i.message}`);
    throw new ConfigError(`Invalid ${CONFIG_FILE}:\n  ${lines.join('\n  ')}`);
  }
  for (const [id, s] of Object.entries(result.data.specs.items)) {
    if (!new RegExp(result.data.specs.idPattern).test(id))
      throw new ConfigError(`Invalid ${CONFIG_FILE}:\n  specs.items.${id}: does not match specs.idPattern`);
    void s;
  }
  return result.data;
}

export function loadConfig(cwd: string, file = CONFIG_FILE): GuardrailsConfig {
  const path = join(cwd, file);
  if (!existsSync(path)) throw new ConfigError(`No ${file} in ${cwd}. Run "guardrails init" first.`);
  let raw: unknown;
  try {
    raw = JSON.parse(readFileSync(path, 'utf8'));
  } catch (e) {
    throw new ConfigError(`${file} is not valid JSON: ${(e as Error).message}`);
  }
  return parseConfig(raw);
}
