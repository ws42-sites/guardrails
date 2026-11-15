import { existsSync, lstatSync, readlinkSync } from 'node:fs';
import { basename, extname, join } from 'node:path';
import type { CheckContext, CheckOutput, Violation } from '../types.js';
import { lineCount, listFiles, matchesAny, readText, violation } from '../util.js';

/** DOC-1 allowlist (incl. doc-in-disguise), DOC-2 one agent file, DOC-3 length caps. */
export function checkDocs(ctx: CheckContext, files = listFiles(ctx.cwd)): CheckOutput {
  const { docs } = ctx.config;
  const v: Violation[] = [];
  const exts = docs.extensions.map((e) => e.toLowerCase());
  const pointers = new Set(docs.pointerFiles);

  for (const f of files) {
    if (matchesAny(f, docs.ignore) || pointers.has(f)) continue;
    const isDoc = exts.includes(extname(f).toLowerCase());
    const disguised = matchesAny(f, docs.disguise);
    if (!isDoc && !disguised) continue;
    if (matchesAny(f, docs.allow) || f === docs.agentFile) {
      const cap = docs.maxLines[f] ?? docs.maxLines[basename(f)] ?? docs.maxLines.default;
      const n = lineCount(readText(ctx.cwd, f));
      if (cap && n > cap) v.push(violation('DOC-3', f, `${n} lines (cap ${cap}). Move rules into checks and trim it.`, 'cap'));
      continue;
    }
    v.push(
      violation('DOC-1', f, disguised && !isDoc
        ? 'looks like a doc in disguise (notes/todo/plan file). Put it in an allowlisted doc or delete it.'
        : `not in docs.allow. Put the content in ${docs.agentFile} or docs/ARCHITECTURE.md (or a check), then delete it.`, 'file'),
    );
  }

  const agent = docs.agentFile;
  if (!existsSync(join(ctx.cwd, agent))) v.push(violation('DOC-2', agent, `missing ${agent}, the canonical agent instruction file. Run "guardrails init".`, 'missing'));
  for (const p of docs.pointerFiles) {
    const abs = join(ctx.cwd, p);
    if (!existsSync(abs)) continue;
    if (lstatSync(abs).isSymbolicLink() && basename(readlinkSync(abs)) === basename(agent)) continue;
    const body = readText(ctx.cwd, p).trim();
    const oneLine = body.split('\n').filter((l) => l.trim()).length <= 1;
    if (!oneLine || !body.includes(basename(agent)))
      v.push(violation('DOC-2', p, `must be a one-line pointer to ${agent} (e.g. "See ${agent}."), or a symlink to it. Move its content into ${agent}.`, 'pointer'));
  }

  for (const [doc, links] of Object.entries(docs.requiredLinks)) {
    if (!existsSync(join(ctx.cwd, doc))) continue;
    const body = readText(ctx.cwd, doc);
    for (const l of links) if (!body.includes(l)) v.push(violation('DOC-1', doc, `must link to ${l}.`, `link:${l}`));
  }
  return { violations: v };
}
