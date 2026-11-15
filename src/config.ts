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
          .record(z.object({ tier, behaviour: z.string().min(3) }).strict())
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
    deps: z.object({ allow: z.array(z.string()).default([]) }).strict().default({}),
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
      })
      .strict()
      .default({}),
    mutation: z
      .object({
        tiers: z.array(tier).default(['T1']),
        break: z.number().min(0).max(100).default(70),
        timeoutMinutes: z.number().positive().default(15),
        excludedMutations: z.array(z.string()).default([]),
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
    labels: z
      .object({ rebaseline: z.string().default('guardrails:rebaseline') })
      .strict()
      .default({}),
  })
  .strict();

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
