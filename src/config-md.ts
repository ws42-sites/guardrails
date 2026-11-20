import { ConfigError, parseConfig, type GuardrailsConfig } from './config.js';

type ObjectValue = Record<string, unknown>;
const object = (value: unknown): value is ObjectValue => value !== null && typeof value === 'object' && !Array.isArray(value);

export function mergeConfig(lower: ObjectValue, upper: ObjectValue): ObjectValue {
  const result = { ...lower };
  for (const [key, value] of Object.entries(upper)) {
    const previous = Object.hasOwn(result, key) ? result[key] : undefined;
    Object.defineProperty(result, key, { value: object(previous) && object(value) ? mergeConfig(previous, value) : value, enumerable: true, configurable: true, writable: true });
  }
  return result;
}

const invalid = (line: number, reason: string): never => { throw new ConfigError(`Invalid guardrails.md at line ${line}: ${reason}`); };
const allowed = (key: string, line: number): void => {
  if (['journeys', 'deps'].includes(key)) invalid(line, `${key} belongs in guardrails.config.json`);
};

function valueAt(text: string, line: number): unknown {
  if (/^(true|false|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?)$/.test(text) || /^["[{]/.test(text)) {
    try { return JSON.parse(text); } catch { return invalid(line, 'invalid JSON value'); }
  }
  if (!text || /^['|>]/.test(text)) return invalid(line, 'expected a boolean, number, string or inline JSON');
  return text;
}

export function parseConfigMarkdown(source: string): ObjectValue {
  const lines = source.split(/\r?\n/);
  let front: ObjectValue = {};
  let blocks: ObjectValue = {};
  let i = 0;
  if (lines[0].trim() === '---') {
    for (i = 1; i < lines.length && lines[i].trim() !== '---'; i++) {
      const text = lines[i].trim();
      if (!text || text.startsWith('#')) continue;
      const match = /^([A-Za-z_$][\w$-]*(?:\.[A-Za-z_$][\w$-]*)*):\s*(.*)$/.exec(text);
      if (!match) return invalid(i + 1, 'expected dotted.key: value');
      const keys = match[1].split('.');
      allowed(keys[0], i + 1);
      let value = valueAt(match[2], i + 1);
      for (const key of keys.reverse()) value = { [key]: value };
      front = mergeConfig(front, value as ObjectValue);
    }
    if (i === lines.length) return invalid(1, 'unclosed front matter');
    i++;
  }
  for (; i < lines.length; i++) {
    const fence = /^\s*(`{3,}|~{3,})(.*)$/.exec(lines[i]);
    if (!fence) continue;
    const start = i + 1;
    const info = fence[2].trim();
    const active = info === 'guardrails' || info === 'json guardrails';
    const body: string[] = [];
    const close = new RegExp(`^\\s*${fence[1][0]}{${fence[1].length},}\\s*$`);
    for (i++; i < lines.length && !close.test(lines[i]); i++) body.push(lines[i]);
    if (!active) continue;
    if (i === lines.length) return invalid(start, 'unclosed guardrails fence');
    let raw: unknown;
    try { raw = JSON.parse(body.join('\n')); } catch { return invalid(start, 'invalid JSON block'); }
    if (!object(raw)) return invalid(start, 'guardrails block must be a JSON object');
    for (const key of Object.keys(raw)) allowed(key, start);
    blocks = mergeConfig(blocks, raw);
  }
  return mergeConfig(blocks, front);
}

export function configFromSources(markdown: string | null, json: string | null): GuardrailsConfig {
  let raw = markdown === null ? {} : parseConfigMarkdown(markdown);
  if (json !== null) {
    let parsed: unknown;
    try { parsed = JSON.parse(json); } catch { throw new ConfigError('guardrails.config.json is not valid JSON'); }
    if (!object(parsed)) throw new ConfigError('guardrails.config.json must be a JSON object');
    raw = mergeConfig(raw, parsed);
  }
  return parseConfig({ version: 1, ...raw });
}
