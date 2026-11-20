import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { parseConfig } from '../../src/config.js';
export { afterEach, beforeEach, expect, it, vi };
export const now = new Date('2026-01-08T00:00:00.000Z');
export const config = parseConfig({ version: 1 });
export function fixture(): string {
  const dir = mkdtempSync(join(tmpdir(), 'eval-'));
  afterEach(() => rmSync(dir, { recursive: true, force: true }));
  return dir;
}
