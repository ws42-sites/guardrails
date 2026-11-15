import { describe, expect, it } from 'vitest';
import { checkDuplication } from '../src/checks/dup.js';
import { listFiles } from '../src/util.js';
import { repo } from './helpers/repo.js';

const fn = (name: string) => `export function ${name}(items: { price: number; qty: number }[], rate: number) {
  let total = 0;
  for (const item of items) {
    const line = item.price * item.qty;
    total += line - line * rate;
  }
  const rounded = Math.round(total * 100) / 100;
  return rounded > 0 ? rounded : 0;
}
`;

describe('DUP-01 copy-paste in new product code blocks above duplication.maxPercent', () => {
  it('blocks a pasted copy of existing code and passes new original code', () => {
    const r = repo({ 'src/lib/a.ts': fn('total') }).commit();
    r.write({ 'src/lib/b.ts': fn('sum') });
    const out = checkDuplication(r.ctx('main'), listFiles(r.dir));
    expect(out.violations.map((v) => [v.rule, v.file, v.message.split(' (')[0]])).toEqual([['ARCH-8', 'src/lib/b.ts', '9 added lines are copies of src/lib/a.ts:1']]);
    r.write({ 'src/lib/b.ts': 'export const twice = (n: number) => n * 2;\n' });
    expect(checkDuplication(r.ctx('main'), listFiles(r.dir))).toEqual({ violations: [], metrics: [{ name: 'duplication.newCode', value: '0%', detail: '0 of 1 added product lines duplicated' }] });
  });
});
