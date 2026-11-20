import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import type { CheckContext, CheckOutput, Violation } from './types.js';
import { mergeBase, showAt, violation } from './util.js';
import { LEGACY_LABELS, hasLabel } from './labels.js';

export interface Flag { id: string; specId: string; file: string; reason: string; createdAt: string; by?: string }
export interface FlagFile { version: 1; flags: Flag[] }

export function readFlags(cwd: string, file: string): FlagFile {
  const p = join(cwd, file);
  if (!existsSync(p)) return { version: 1, flags: [] };
  const raw = readFileSync(p, 'utf8').trim();
  if (!raw) return { version: 1, flags: [] };
  const j = JSON.parse(raw) as Partial<FlagFile>;
  return { version: 1, flags: j.flags ?? [] };
}

/** Append a flag. Agents call this (MCP guardrails_flag / `guardrails flag`) instead of bending code or tests. */
export function addFlag(cwd: string, file: string, input: { specId: string; file: string; reason: string; by?: string }, now: Date = new Date()): Flag {
  if (!input.reason || input.reason.trim().length < 10) throw new Error('reason must explain the conflict (at least 10 characters)');
  const data = readFlags(cwd, file);
  const flag: Flag = { id: `F${data.flags.length + 1}`, specId: input.specId, file: input.file, reason: input.reason.trim(), createdAt: now.toISOString(), by: input.by };
  data.flags.push(flag);
  const p = join(cwd, file);
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, `${JSON.stringify(data, null, 2)}\n`);
  return flag;
}

/**
 * FLAG-1: open flags block merge until a human resolves them (empty the file, or the flags-approved
 * label). Clearing flags in a PR warns so the code owner reviewing .guardrails/ sees it.
 */
export function checkFlags(ctx: CheckContext): CheckOutput {
  const { flagsFile, labels } = ctx.config;
  const resolved = hasLabel(ctx.labels, labels.flagsResolved, LEGACY_LABELS.flags);
  const v: Violation[] = [];
  let flags: Flag[] = [];
  try {
    flags = readFlags(ctx.cwd, flagsFile).flags;
  } catch {
    v.push(violation('FLAG-1', flagsFile, 'is not valid JSON. A human must repair or empty it.', 'invalid'));
  }
  if (!resolved)
    for (const f of flags)
      v.push(violation('FLAG-1', f.file || flagsFile, `open flag ${f.id} on ${f.specId}: ${f.reason}. A human must resolve it before merge.`, `flag:${f.specId}:${f.reason}`));
  if (ctx.base && !resolved) {
    const before = showAt(ctx.cwd, mergeBase(ctx.cwd, ctx.base), flagsFile);
    if (before?.trim()) {
      const old = (JSON.parse(before) as FlagFile).flags ?? [];
      const now = new Set(flags.map((f) => `${f.specId}:${f.reason}`));
      for (const f of old)
        if (!now.has(`${f.specId}:${f.reason}`))
          v.push({ ...violation('FLAG-1', flagsFile, `flag ${f.id} (${f.specId}) was cleared in this PR. Only a human clears flags; a code owner must review ${flagsFile} (or add the "${labels.flagsResolved}" label, which says a human has answered the open flags).`, `cleared:${f.specId}:${f.reason}`), maxMode: 'warn' });
    }
  }
  return { violations: v, metrics: flags.length ? [{ name: 'flags.open', value: flags.length, detail: flags.map((f) => `${f.id} ${f.specId} ${f.file}: ${f.reason}`).join('\n') }] : [] };
}
