import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { z } from 'zod';
import { configFromSources } from './config-md.js';
import { RULE_IDS } from './rules.js';

function lazySchema<T extends z.ZodTypeAny>(build: () => T): T {
  let schema: T | undefined;
  const get = (): T => (schema ??= build());
  return new Proxy({} as T, {
    get(_target, prop) {
      const resolved = get();
      const value = Reflect.get(resolved, prop, resolved);
      return typeof value === 'function' ? value.bind(resolved) : value;
    },
    has(_target, prop) {
      return Reflect.has(get(), prop);
    },
    ownKeys() {
      return Reflect.ownKeys(get());
    },
    getOwnPropertyDescriptor(_target, prop) {
      const desc = Object.getOwnPropertyDescriptor(get(), prop);
      if (desc) desc.configurable = true;
      return desc;
    },
    getPrototypeOf() {
      return Object.getPrototypeOf(get());
    },
  });
}

function buildSchemas() {
  const globs = z.array(z.string().min(1));
const tier = z.enum(['T1', 'T2', 'T3']);
const mode = z.enum(['block', 'warn', 'off']);

const cujString = z.string().min(1).max(300).refine((s) => s.trim().length > 0, 'must not be blank');
const token = /\{\{[^{}]*\}\}/g;
const pathValue = cujString.refine((s) => s.startsWith('/') || /^\{\{[^{}]+\}\}/.test(s), 'path must start with /');
const selector = cujString.refine((s) => /^[A-Za-z0-9_\-\[\]=^$*"'.#:() >]+$/.test(s.replace(token, 'token')), 'unsafe selector');
const locator = z.object({ role: cujString.optional(), name: cujString.optional(), text: cujString.optional() }).strict()
  .refine((s) => !!s.text || !!(s.role && s.name), 'provide role and name, or text');

/** Each strict branch is discriminated by its sole operation key; no executable code. */
const journeyStepSchema = z.union([
  z.object({ goto: pathValue }).strict(),
  z.object({ click: locator }).strict(),
  z.object({ fill: z.object({ label: cujString, value: cujString }).strict() }).strict(),
  z.object({ submit: selector }).strict(),
  z.object({ expectText: cujString }).strict(),
  z.object({ expectUrl: pathValue }).strict(),
  z.object({ expectVisible: locator }).strict(),
  z.object({ expectHref: z.object({ selector, value: cujString }).strict() }).strict(),
  z.object({ expectNoHorizontalScroll: z.literal(true) }).strict(),
  z.object({ viewport: z.object({ width: z.number().int().min(200).max(4000), height: z.number().int().min(200).max(4000) }).strict() }).strict(),
  z.object({ mockRoute: z.object({ match: cujString, status: z.number().int().min(100).max(599) }).strict() }).strict(),
]);

const journeySchema = z.object({
  title: z.string().min(10).max(300),
  priority: z.enum(['P0', 'P1', 'P2']),
  steps: z.array(z.string().min(3).max(300)).min(1),
  specs: z.array(z.string()).default([]),
  routes: z.array(z.string()).default([]),
  approved: z.boolean().default(false),
  approvedBy: z.string().optional(),
  persona: cujString.optional(),
  goal: cujString.optional(),
  success: cujString.optional(),
  provenance: z.enum(['client-provided', 'inferred', 'template']).optional(),
  source: z.object({ by: cujString, runId: cujString.optional(), inputs: z.array(cujString).optional() }).strict().optional(),
  run: z.array(journeyStepSchema).max(40).optional(),
}).strict().superRefine((j, ctx) => {
  const metadata = [j.approvedBy, ...j.specs, ...j.routes, j.source?.by, j.source?.runId, ...(j.source?.inputs ?? [])];
  if (metadata.some((s) => s?.includes('{{'))) ctx.addIssue({ code: 'custom', message: 'tokens are only legal in run values and prose' });
}).transform((j) => j.source && !j.provenance ? { ...j, provenance: 'inferred' as const } : j);

const configSchema = z
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
        path: z.string().optional(),
        specs: z.array(z.string()).default([]),
        target: z.string().default('test/holdout'),
      })
      .strict()
      .optional(),
    lock: z
      .object({
        specTests: z.boolean().default(true),
        paths: globs.default(['guardrails.config.json', 'guardrails.md', '.guardrails/**', '.github/CODEOWNERS', '.github/workflows/**', '.claude/settings.json', '.gemini/settings.json', '.agents/hooks.json', '.codex/hooks.json', '.codex/config.toml', '.gitleaks.toml', '.gitleaksignore']),
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
          dataFile: z.string().default('.guardrails/client-data.json'),
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
        trustedProducers: z.array(cujString).default([]),
        items: z.record(journeySchema).default({}),
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
        testsRemovedLabel: z.string().default('guardrails:tests-removed-approved'),
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
        allow: globs.default(['AGENTS.md', 'CLAUDE.md', 'GEMINI.md', 'README.md', 'ARCHITECTURE.md', 'TESTING.md', 'RUNBOOK.md']),
        /** Markdown the app loads at runtime (skills, prompt files). Allowed by DOC-1, exempt from DOC-3. */
        runtime: globs.default([]),
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
    runbook: z
      .object({
        /** RUN-1..4. Off until a repo adopts the standard ("guardrails init runbook" turns it on). */
        enabled: z.boolean().default(false),
        file: z.string().default('RUNBOOK.md'),
        maxLines: z.number().int().positive().default(150),
        packageJson: z.string().default('package.json'),
        /** Standard scripts that genuinely do not apply, with the reason (RUN-2). */
        na: z.record(z.enum(['deploy', 'deploy:preview', 'rollback', 'smoke', 'secrets:check']), z.string().min(10)).default({}),
        workflows: globs.default(['.github/workflows/*.yml', '.github/workflows/*.yaml']),
        /** Other runbook/deploy docs tolerated by RUN-4 (e.g. legacy docs pending a cleanup). */
        allowDocs: globs.default([]),
      })
      .strict()
      .default({}),
    ci: z
      .object({
        /** CI-1. Off until a repo adopts change-scoped CI. */
        enabled: z.boolean().default(false),
        workflows: globs.default(['.github/workflows/*.yml', '.github/workflows/*.yaml']),
        /** Limit which test files `affected` can select (e.g. the root node:test suite); empty = all test files. */
        tests: globs.default([]),
        /** Changes to these select every test (guardrails affected). */
        globalFiles: globs.default(['package.json', 'package-lock.json', '**/tsconfig*.json', 'vitest.config.*', 'vitest.setup.*', 'playwright.config.*', '.nvmrc']),
        /** Safety nets that genuinely do not apply, with the reason. */
        na: z.object({ nightly: z.string().min(10).optional(), preDeploy: z.string().min(10).optional() }).strict().default({}),
      })
      .strict()
      .default({}),
    testSuite: z
      .object({
        /** SUITE-1..6 (guardrails check-tests). Off until a repo commits a baseline. */
        enabled: z.boolean().default(false),
        maxLines: z.number().int().positive().default(300),
        baselineFile: z.string().default('.guardrails/test-suite-baseline.json'),
        /** Paths that are neither tests nor product for the audit (generated code, fixtures). */
        exclude: globs.default([]),
      })
      .strict()
      .default({}),
    review: z
      .object({
        enabled: z.boolean().default(false),
        provider: z.enum(['stub', 'command']).default('stub'),
        command: z.string().min(1).optional(),
        timeoutSeconds: z.number().int().positive().default(300),
        maxDiffBytes: z.number().int().positive().default(200000),
        stub: z.object({ failOn: z.array(z.string().min(1)).default([]) }).strict().default({}),
        paths: globs.default(['**/auth/**', '**/*auth*.*', '**/*session*.*', '**/*token*.*']),
        onlyWhenRisky: z.boolean().default(true),
      })
      .strict()
      .default({})
      .superRefine((r, ctx) => {
        if (r.enabled && r.provider === 'command' && !r.command) ctx.addIssue({ code: 'custom', path: ['command'], message: 'review.provider "command" needs review.command' });
      }),
    provenance: z
      .object({
        enabled: z.boolean().default(false),
        out: z.string().min(1).default('guardrails-provenance.json'),
        overrideLabelPrefix: z.string().min(1).default('guardrails:'),
      })
      .strict()
      .default({}),
    evaluation: z
      .object({
        enabled: z.boolean().default(false),
        ledgerFile: z.string().min(1).default('.guardrails/ledger.jsonl'),
        escapedLabel: z.string().min(1).default('guardrails:escaped-bug'),
        windowDays: z.number().int().positive().default(7),
        minFalseEscalations: z.number().int().positive().default(3),
      })
      .strict()
      .default({}),
    feedback: z
      .object({
        enabled: z.boolean().default(false),
        logFile: z.string().min(1).default('guardrails-failures.jsonl'),
        minRepeats: z.number().int().positive().default(3),
      })
      .strict()
      .default({}),
    telemetry: z
      .object({
        enabled: z.boolean().default(false),
        endpoint: z.string().url().optional(),
        timeoutMs: z.number().int().positive().default(1500),
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
        /** A code owner approved growing the baseline (BASE-1, SUITE-6). */
        rebaseline: z.string().default('guardrails:baseline-approved'),
        /** A code owner approved new tests that lock in existing behaviour and so already pass on base (TEST-11). */
        characterization: z.string().default('guardrails:existing-behaviour-tests-approved'),
        /** A code owner resolved the open flags (FLAG-1). */
        flagsResolved: z.string().default('guardrails:flags-approved'),
        /** A code owner approved loosening guardrails.config.json in this PR (TEST-7 config part). */
        configApproved: z.string().default('guardrails:config-approved'),
        /** A code owner approved the journey changes in this PR (JRN-3). */
        journeysApproved: z.string().default('guardrails:journeys-approved'),
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

  return { journeyStepSchema, journeySchema, configSchema };
}

let cachedSchemas: ReturnType<typeof buildSchemas> | undefined;
const schemas = () => (cachedSchemas ??= buildSchemas());

export const journeyStepSchema = lazySchema(() => schemas().journeyStepSchema);
export const journeySchema = lazySchema(() => schemas().journeySchema);
export const configSchema = lazySchema(() => schemas().configSchema);

export type GuardrailsConfig = z.output<typeof configSchema>;
export const CONFIG_FILE = 'guardrails.config.json';
export const CONFIG_MD = 'guardrails.md';

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
  const markdown = join(cwd, CONFIG_MD);
  if (!existsSync(path) && !existsSync(markdown)) throw new ConfigError(`No ${file} in ${cwd}. Run "guardrails init" first.`);
  return configFromSources(existsSync(markdown) ? readFileSync(markdown, 'utf8') : null, existsSync(path) ? readFileSync(path, 'utf8') : null);
}
