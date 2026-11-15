import type { Rule } from 'eslint';

/** Custom ESLint rules used by TEST-5. Everything else comes from eslint-plugin-vitest and core rules. */
const noStrykerDisable: Rule.RuleModule = {
  meta: { type: 'problem', schema: [], messages: { banned: '"Stryker disable" comments are banned; they hide surviving mutants. Kill the mutant with a real assertion.' } },
  create(context) {
    return {
      Program() {
        for (const c of context.sourceCode.getAllComments())
          if (/stryker\s+disable/i.test(c.value)) context.report({ loc: c.loc ?? { line: 1, column: 0 }, messageId: 'banned' });
      },
    };
  },
};

const LIMIT = 20;
const noGeneratedInputs: Rule.RuleModule = {
  meta: {
    type: 'problem',
    schema: [],
    messages: {
      loop: 'Loop over {{n}} iterations in a test. Generated input loops are padding; use a small it.each table of meaningful cases.',
      fc: 'fast-check run without numRuns <= ' + LIMIT + '. Property loops are padding here; use fixed cases or cap numRuns.',
    },
  },
  create(context) {
    const literal = (n: unknown): number | null => {
      const x = n as { type?: string; value?: unknown };
      return x && x.type === 'Literal' && typeof x.value === 'number' ? x.value : null;
    };
    return {
      ForStatement(node) {
        const t = node.test as unknown as { type: string; operator?: string; right?: unknown };
        if (t && t.type === 'BinaryExpression' && (t.operator === '<' || t.operator === '<=')) {
          const n = literal(t.right);
          if (n !== null && n > LIMIT) context.report({ node, messageId: 'loop', data: { n: String(n) } });
        }
      },
      'CallExpression[callee.object.name="Array"][callee.property.name="from"]'(node: Rule.Node) {
        const arg = (node as unknown as { arguments: { type: string; properties?: { key?: { name?: string }; value?: unknown }[] }[] }).arguments[0];
        const len = arg?.type === 'ObjectExpression' ? arg.properties?.find((p) => p.key?.name === 'length') : undefined;
        const n = len ? literal(len.value) : null;
        if (n !== null && n > LIMIT) context.report({ node, messageId: 'loop', data: { n: String(n) } });
      },
      'CallExpression[callee.object.name="fc"][callee.property.name="assert"]'(node: Rule.Node) {
        const args = (node as unknown as { arguments: { type: string; properties?: { key?: { name?: string }; value?: unknown }[] }[] }).arguments;
        const opts = args[1];
        const runs = opts?.type === 'ObjectExpression' ? opts.properties?.find((p) => p.key?.name === 'numRuns') : undefined;
        const n = runs ? literal(runs.value) : null;
        if (n === null || n > LIMIT) context.report({ node, messageId: 'fc' });
      },
    };
  },
};

export const plugin = { rules: { 'no-stryker-disable': noStrykerDisable, 'no-generated-inputs': noGeneratedInputs } };

/** Restricted syntax for every test file. `tag` lets the checker apply file-level exemptions. */
export const TEST_SYNTAX: { selector: string; message: string; tag?: 'clock' | 'random' }[] = [
  { selector: "CallExpression[callee.property.name='todo']", message: 'test.todo is banned; write the test or track it in an issue.' },
  { selector: "CallExpression[callee.property.name='each'][arguments.0.type='ArrayExpression'][arguments.0.elements.length=0]", message: 'each([]) with an empty table runs nothing.' },
  { selector: "CallExpression[callee.property.name=/^(toHaveClass|toHaveStyle)$/]", message: 'Class-name/style assertions are banned; visuals are checked by screenshot diff.' },
  { selector: "CallExpression[callee.property.name='createServer']", message: 'Hand-rolled fake servers are banned; test through the storage interface or a fixture implementation.' },
  { selector: "CallExpression[callee.object.name='vi'][callee.property.name='spyOn'][arguments.0.name=/^(Date|Response)$/]", message: 'Spying on Date/Response is banned; use vi.useFakeTimers()/setSystemTime or a real Response.' },
  { selector: "CallExpression[callee.object.name='vi'][callee.property.name='spyOn'][arguments.0.type='MemberExpression'][arguments.0.property.name=/^(subtle|prototype)$/]", message: 'Spying on crypto.subtle or a prototype is banned; test the real behaviour.' },
  { selector: "NewExpression[callee.name='Date'][arguments.length=0]", message: 'Real clock in a test: use vi.useFakeTimers() and vi.setSystemTime(new Date("2026-01-01")).', tag: 'clock' },
  { selector: "CallExpression[callee.object.name='Date'][callee.property.name='now']", message: 'Real clock in a test: use vi.useFakeTimers() and vi.setSystemTime().', tag: 'clock' },
  { selector: "CallExpression[callee.object.name='performance'][callee.property.name='now']", message: 'Real clock in a test: use fake timers.', tag: 'clock' },
  { selector: "CallExpression[callee.object.name='process'][callee.property.name='hrtime']", message: 'Real clock in a test: use fake timers.', tag: 'clock' },
  { selector: "MemberExpression[object.name='hrtime'][property.name='bigint']", message: 'Real clock in a test: use fake timers.', tag: 'clock' },
  { selector: "CallExpression[callee.object.name='Math'][callee.property.name='random']", message: 'Randomness in a test: use fixed inputs (or a seeded generator).', tag: 'random' },
  { selector: "CallExpression[callee.property.name=/^(randomUUID|getRandomValues)$/]", message: 'Randomness in a test: use fixed IDs/bytes.', tag: 'random' },
];
export const SNAPSHOT_SYNTAX = {
  selector: "CallExpression[callee.property.name=/^(toMatchSnapshot|toMatchInlineSnapshot|toMatchFileSnapshot|toThrowErrorMatchingSnapshot|toThrowErrorMatchingInlineSnapshot)$/]",
  message: 'Snapshots are banned in T1 tests; assert the exact values that matter.',
};
