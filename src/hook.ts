import { existsSync, readFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { parseConfig, loadConfig as loadProjectConfig, CONFIG_FILE, CONFIG_MD, type GuardrailsConfig } from './config.js';
import { isTestFile, matchesAny } from './util.js';

export type Agent = 'claude' | 'gemini' | 'antigravity' | 'codex';
export interface GuardResult { allow: boolean; reason?: string; path?: string }

const WRITE_SHELL = /(\bsed\s+-i|\bperl\s+-i|>{1,2}|\btee\b|\bmv\b|\brm\b|\bcp\b|\btruncate\b|\bgit\s+(checkout|restore|rm|mv)\b|\bdd\b|\bchmod\b|writeFile|open\([^)]*['"]w)/;
export const UNLOCK_ENV = 'GUARDRAILS_ALLOW_TEST_EDITS';

interface Payload {
  tool_name?: string;
  tool_input?: { file_path?: string; notebook_path?: string; path?: string; command?: string };
  toolCall?: { name?: string; args?: { TargetFile?: string; CommandLine?: string } };
  workspacePaths?: string[];
  cwd?: string;
}

function loadConfig(cwd: string): GuardrailsConfig {
  try {
    return loadProjectConfig(cwd);
  } catch {
    return parseConfig({ version: 1 });
  }
}

/** Nearest ancestor of `from` that holds the guardrails config, else `from` itself (agents can start in a subfolder). */
function configRoot(from: string): string {
  for (let d = resolve(from); ; d = dirname(d)) {
    if (existsSync(join(d, CONFIG_FILE)) || existsSync(join(d, CONFIG_MD))) return d;
    if (dirname(d) === d) return from;
  }
}

export function projectRoot(p: Payload, env: NodeJS.ProcessEnv): string {
  return env.CLAUDE_PROJECT_DIR ?? env.GEMINI_PROJECT_DIR ?? p.workspacePaths?.[0] ?? configRoot(p.cwd ?? process.cwd());
}

/** Files a Codex apply_patch touches: `*** Add|Update|Delete File: path` and `*** Move to: path` headers. */
export function patchPaths(patch: string): string[] {
  return [...patch.matchAll(/^\*\*\* (?:(?:Add|Update|Delete) File|Move to): (.+?)\s*$/gm)].map((m) => m[1]);
}

/** Is this repo-relative path protected from agent edits? */
export function isProtected(cwd: string, cfg: GuardrailsConfig, rel: string): string | null {
  if (matchesAny(rel, cfg.lock.paths)) return 'guardrails config, baseline, flags, CODEOWNERS, CI and hook settings are locked';
  if (cfg.lock.specTests && isTestFile(cfg, rel) && existsSync(join(cwd, rel))) {
    const id = new RegExp(cfg.specs.idPattern.replace(/^\^|\$$/g, ''));
    if (id.test(readFileSync(join(cwd, rel), 'utf8'))) return 'existing spec-tagged test files are locked during builds';
  }
  return null;
}

/** Decide whether an agent tool call may run. Never throws: a broken guard must not wedge the agent. */
export function guard(payload: Payload, env: NodeJS.ProcessEnv = process.env): GuardResult {
  if (env[UNLOCK_ENV] === '1') return { allow: true };
  const cwd = projectRoot(payload, env);
  const cfg = loadConfig(cwd);
  const base = payload.cwd ?? cwd;
  const rel = (p: string) => relative(cwd, isAbsolute(p) ? p : resolve(base, p)).split('\\').join('/').replace(/^\.\//, '');
  const target = payload.tool_input?.file_path ?? payload.tool_input?.notebook_path ?? payload.tool_input?.path ?? payload.toolCall?.args?.TargetFile;
  const command = payload.tool_input?.command ?? payload.toolCall?.args?.CommandLine;
  const tip = `If the test or spec is wrong, STOP and call guardrails_flag. Humans can unlock with ${UNLOCK_ENV}=1.`;
  if (payload.tool_name === 'apply_patch' && command) {
    for (const file of patchPaths(command)) {
      const r = rel(file);
      const why = isProtected(cwd, cfg, r);
      if (why) return { allow: false, path: r, reason: `guardrails: ${r} is protected (${why}). ${tip}` };
    }
    return { allow: true };
  }
  if (target) {
    const r = rel(target);
    const why = isProtected(cwd, cfg, r);
    if (why) return { allow: false, path: r, reason: `guardrails: ${r} is protected (${why}). ${tip}` };
  }
  if (command && (/\bguardrails\s+journeys\s+(approve|draft)\b|\binit\b.*--preset/.test(command) || new RegExp(`${UNLOCK_ENV}|unset\\s+GUARDRAILS_AGENT|GUARDRAILS_AGENT=\\s*(\\s|$)`).test(command)))
    return { allow: false, reason: `guardrails: agents may not add, approve or unlock journeys, tests or guardrails. ${tip}` };
  if (command && WRITE_SHELL.test(command) && !/\bguardrails\s+flag\b/.test(command)) {
    for (const token of command.split(/[\s'"`;|&()<>]+/).filter((t) => /[./]/.test(t))) {
      const r = rel(token);
      const why = isProtected(cwd, cfg, r);
      if (why) return { allow: false, path: r, reason: `guardrails: this command writes to ${r} (${why}). ${tip}` };
    }
  }
  return { allow: true };
}

/** Render the result in each agent's hook protocol. Codex, like Claude Code, blocks on exit 2 with the reason on stderr. */
export function hookResponse(agent: Agent, r: GuardResult): { code: number; stdout: string; stderr: string } {
  if (agent === 'antigravity') return { code: 0, stdout: JSON.stringify(r.allow ? { decision: 'allow' } : { decision: 'deny', reason: r.reason }), stderr: '' };
  if (r.allow) return { code: 0, stdout: agent === 'gemini' ? '{}' : '', stderr: '' };
  return { code: 2, stdout: '', stderr: r.reason ?? 'guardrails: blocked' };
}
