import { spawnSync } from 'node:child_process';

export interface RunResult { code: number; stdout: string; stderr: string }

/** Run a shell command in the project. Output is captured, never streamed into reports verbatim. */
export function runCommand(cmd: string, cwd: string, env: Record<string, string> = {}, timeoutMs = 15 * 60_000): RunResult {
  const r = spawnSync(cmd, { cwd, shell: true, encoding: 'utf8', env: { ...process.env, ...env }, maxBuffer: 256 << 20, timeout: timeoutMs });
  return { code: r.status ?? 1, stdout: r.stdout ?? '', stderr: r.stderr ?? (r.error ? String(r.error) : '') };
}
