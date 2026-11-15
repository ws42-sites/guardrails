import { createHash } from 'node:crypto';
import { parse, type TSESTree } from '@typescript-eslint/typescript-estree';

export interface TestCase {
  title: string;
  /** Spec ID from the test title, else from the nearest describe title. */
  specId: string | null;
  /** Leading token that looks like an ID but fails idPattern (malformed). */
  malformedId: string | null;
  line: number;
  expects: number;
  matchers: string[];
  each: boolean;
  /** Hash of the test's source text: a changed test keeps its title but changes hash. */
  hash: string;
}

export interface Inventory {
  tests: TestCase[];
  parseError?: string;
}

const TEST_FNS = new Set(['it', 'test']);
const SUITE_FNS = new Set(['describe', 'suite']);
const ID_LIKE = /^\[?([A-Za-z][\w]*-\d+)\]?[\s:]/;

function baseName(callee: TSESTree.Node): { name: string; each: boolean } | null {
  // it(...), it.only(...), it.each(table)(...), test.concurrent.each(t)(...)
  let node: TSESTree.Node = callee;
  let each = false;
  if (node.type === 'CallExpression') {
    node = node.callee;
    each = true;
  }
  while (node.type === 'MemberExpression') {
    if (node.property.type === 'Identifier' && node.property.name === 'each') each = true;
    node = node.object;
  }
  if (node.type === 'TaggedTemplateExpression') {
    each = true;
    return baseName(node.tag) && { name: (baseName(node.tag) as { name: string }).name, each };
  }
  return node.type === 'Identifier' ? { name: node.name, each } : null;
}

function titleOf(arg: TSESTree.Node | undefined): string | null {
  if (!arg) return null;
  if (arg.type === 'Literal' && typeof arg.value === 'string') return arg.value;
  if (arg.type === 'TemplateLiteral') return arg.quasis.map((q) => q.value.cooked).join('${}');
  return null;
}

function idFrom(title: string | null, pattern: RegExp): { id: string | null; malformed: string | null } {
  if (!title) return { id: null, malformed: null };
  const m = ID_LIKE.exec(title.trimStart() + ' ');
  if (!m) return { id: null, malformed: null };
  return pattern.test(m[1]) ? { id: m[1], malformed: null } : { id: null, malformed: m[1] };
}

function collectExpects(node: TSESTree.Node | undefined, out: { expects: number; matchers: string[] }): void {
  if (!node) return;
  const visit = (n: unknown): void => {
    if (!n || typeof n !== 'object') return;
    const t = n as TSESTree.Node;
    if (t.type === 'CallExpression') {
      // expect(x).not.toBe(y)  -> callee chain ends in Identifier expect
      let c: TSESTree.Node = t.callee;
      const props: string[] = [];
      while (c.type === 'MemberExpression') {
        if (c.property.type === 'Identifier') props.unshift(c.property.name);
        c = c.object;
      }
      if (c.type === 'CallExpression' && c.callee.type === 'Identifier' && c.callee.name === 'expect' && props.length) {
        out.expects++;
        const literal = c.arguments[0] && c.arguments[0].type === 'Literal' ? `(${String((c.arguments[0] as TSESTree.Literal).value)})` : '';
        out.matchers.push(props.filter((p) => p !== 'resolves' && p !== 'rejects').join('.') + (literal ? `@expect${literal}` : ''));
      } else if (c.type === 'Identifier' && c.name === 'expect' && props[0] === 'assertions') {
        /* expect.assertions(n) is bookkeeping, not an assertion */
      }
    }
    for (const [k, v] of Object.entries(t)) {
      if (k === 'parent' || k === 'loc' || k === 'range') continue;
      if (Array.isArray(v)) v.forEach(visit);
      else if (v && typeof v === 'object' && 'type' in v) visit(v);
    }
  };
  visit(node);
}

export function inventory(source: string, idPattern: string, file = 'test.ts'): Inventory {
  const pattern = new RegExp(idPattern);
  let ast: TSESTree.Program;
  try {
    ast = parse(source, { loc: true, range: true, jsx: /\.(tsx|jsx)$/.test(file), comment: false });
  } catch (e) {
    return { tests: [], parseError: (e as Error).message };
  }
  const tests: TestCase[] = [];
  const walkNode = (n: unknown, suiteId: string | null): void => {
    if (!n || typeof n !== 'object') return;
    const node = n as TSESTree.Node;
    if (node.type === 'CallExpression') {
      const b = baseName(node.callee);
      if (b && (TEST_FNS.has(b.name) || SUITE_FNS.has(b.name))) {
        const title = titleOf(node.arguments[0]);
        const { id, malformed } = idFrom(title, pattern);
        if (SUITE_FNS.has(b.name)) {
          for (const a of node.arguments.slice(1)) walkNode(a, id ?? suiteId);
          return;
        }
        const body = node.arguments[1] ?? node.arguments[2];
        const acc = { expects: 0, matchers: [] as string[] };
        collectExpects(body, acc);
        tests.push({
          title: title ?? '<dynamic title>',
          specId: id ?? suiteId,
          malformedId: malformed,
          line: node.loc.start.line,
          expects: acc.expects,
          matchers: acc.matchers,
          each: b.each,
          hash: createHash('sha1').update(source.slice(node.range[0], node.range[1])).digest('hex').slice(0, 12),
        });
        return;
      }
    }
    for (const [k, v] of Object.entries(node)) {
      if (k === 'parent' || k === 'loc' || k === 'range') continue;
      if (Array.isArray(v)) v.forEach((x) => walkNode(x, suiteId));
      else if (v && typeof v === 'object' && 'type' in v) walkNode(v, suiteId);
    }
  };
  walkNode(ast, null);
  return { tests };
}

/** Matchers ranked by strength. A drop in rank inside one test is a TEST-7 downgrade. */
export const MATCHER_STRENGTH: Record<string, number> = {
  toStrictEqual: 3, toEqual: 3, toBe: 3, toThrow: 2, toThrowError: 2, toHaveLength: 2, toMatch: 2,
  toHaveBeenCalledWith: 2, toHaveProperty: 2, toBeNull: 2, toBeUndefined: 2, toBeGreaterThan: 2, toBeLessThan: 2,
  toMatchObject: 1, toContain: 1, toContainEqual: 1, toHaveBeenCalled: 1, toHaveBeenCalledTimes: 1,
  toBeTruthy: 0, toBeFalsy: 0, toBeDefined: 0,
};
export const matcherName = (m: string): string => m.split('@')[0].split('.').filter((p) => p !== 'not').pop() ?? m;
