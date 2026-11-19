import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { importJourneys } from '../../src/journeys-import.js';
import { guard } from '../../src/hook.js';
import { repo } from './repo.js';
import { cfg } from '../fixtures/cuj/cases.js';

export function importRepo(input: unknown, config: object = cfg, bad?: unknown) {
  const r = repo({ 'input.json': JSON.stringify(input), 'bad.json': JSON.stringify(bad ?? []) }, config);
  return {
    read: () => readFileSync(join(r.dir, 'guardrails.config.json'), 'utf8'),
    import: () => importJourneys(r.dir, join(r.dir, 'input.json')),
    cli: (file: string) => spawnSync(process.execPath, [resolve('dist/cli.js'), 'journeys', 'import', file], { cwd: r.dir, env: { ...process.env, GUARDRAILS_AGENT: 'test-agent' }, encoding: 'utf8' }),
    allows: (arg: string) => guard({ tool_input: { command: 'npx guardrails journeys ' + arg } }, { CLAUDE_PROJECT_DIR: r.dir }).allow,
  };
}
