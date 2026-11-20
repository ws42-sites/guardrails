import { randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { loadConfig, type GuardrailsConfig } from './config.js';

type Command = 'init' | 'check' | 'report';
interface Input { command: Command; counts: { block: number; warn: number }; durationMs: number }
interface Dependencies {
  cwd: string;
  config?: GuardrailsConfig;
  env?: NodeJS.ProcessEnv;
  fetch?: typeof fetch;
  uuid?: () => string;
  version?: string;
  node?: string;
  os?: NodeJS.Platform;
}
interface Payload extends Input { v: 1; event: Command; version: string; node: number; os: NodeJS.Platform; installId: string }

function reason(config: GuardrailsConfig, env: NodeJS.ProcessEnv): string {
  if (!config.telemetry.enabled) return 'telemetry.enabled is false';
  if (!config.telemetry.endpoint) return 'telemetry.endpoint is missing';
  if (env.GUARDRAILS_TELEMETRY === '0') return 'GUARDRAILS_TELEMETRY=0';
  if (env.CI && env.CI !== '0' && env.CI !== 'false' && env.GUARDRAILS_TELEMETRY !== '1') return 'CI requires GUARDRAILS_TELEMETRY=1';
  return 'opted in';
}

function installId(deps: Dependencies): string {
  const path = join(deps.cwd, 'node_modules', '.cache', 'guardrails', 'install-id');
  const read = () => {
    const id = readFileSync(path, 'utf8').trim();
    if (!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(id)) throw new Error('Invalid telemetry install ID');
    return id;
  };
  try { return read(); } catch {
    const id = (deps.uuid ?? randomUUID)();
    mkdirSync(dirname(path), { recursive: true });
    try { writeFileSync(path, id, { flag: 'wx' }); } catch { return read(); }
    return id;
  }
}

function payload(event: Command, input: Input, deps: Dependencies): Payload {
  return {
    v: 1, event,
    version: deps.version ?? (JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as { version: string }).version,
    node: Number((deps.node ?? process.versions.node).split('.')[0]), os: deps.os ?? process.platform,
    command: input.command, counts: { block: input.counts.block, warn: input.counts.warn }, durationMs: input.durationMs,
    installId: installId(deps),
  };
}

export function telemetryStatus(event: Command, input: Input, deps: Dependencies): { enabled: boolean; reason: string; payload: Payload } {
  const why = reason(deps.config ?? loadConfig(deps.cwd), deps.env ?? process.env);
  return { enabled: why === 'opted in', reason: why, payload: payload(event, input, deps) };
}

export async function send(event: Command, input: Input, deps: Dependencies): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const config = deps.config ?? loadConfig(deps.cwd);
    if (reason(config, deps.env ?? process.env) !== 'opted in') return;
    const body = JSON.stringify(payload(event, input, deps));
    const controller = new AbortController();
    const timeout = new Promise<void>((resolve) => {
      timer = setTimeout(() => { controller.abort(); resolve(); }, config.telemetry.timeoutMs);
      timer.unref();
    });
    await Promise.race([(deps.fetch ?? fetch)(config.telemetry.endpoint as string, {
      method: 'POST', headers: { 'content-type': 'application/json' }, body, signal: controller.signal,
    }), timeout]);
  } catch {} finally { clearTimeout(timer); }
}
