import type { GuardrailsConfig } from './config.js';

type Journey = GuardrailsConfig['journeys']['items'][string];
export class UnresolvedTokensError extends Error {
  constructor(public readonly tokens: string[]) {
    super(`Unresolved or empty tokens: ${tokens.join(', ')}`);
    this.name = 'UnresolvedTokensError';
  }
}

/** Resolve own properties only; never traverse prototypes or recursively expand data. */
export function resolveTokens(value: string, data: Record<string, unknown>): string {
  const missing = new Set<string>();
  const malformed = value.replace(/\{\{[^{}]*\}\}/g, '');
  for (const part of malformed.matchAll(/\{\{[^{}]*(?:$|(?=\{))/g)) missing.add(part[0]);
  const result = value.replace(/\{\{([^{}]*)\}\}/g, (raw, name: string) => {
    let current: unknown = data;
    const path = name.trim();
    for (const key of path.split('.')) {
      if (!key || !current || typeof current !== 'object' || !Object.hasOwn(current, key)) { current = undefined; break; }
      current = (current as Record<string, unknown>)[key];
    }
    if (!['string', 'number', 'boolean'].includes(typeof current) || !String(current).trim() || String(current).includes('{{')) {
      missing.add(path); return raw;
    }
    return String(current);
  });
  if (result.includes('{{') && !missing.size) missing.add(result.slice(result.indexOf('{{')));
  if (missing.size) throw new UnresolvedTokensError([...missing]);
  return result;
}

/** Copy the journey, resolving prose and run values without changing its authority metadata. */
export function resolveJourney<T extends Partial<Journey>>(journey: T, data: Record<string, unknown>): T {
  const missing = new Set<string>();
  const walk = (value: unknown): unknown => {
    if (typeof value === 'string') {
      try { return resolveTokens(value, data); }
      catch (e) { if (!(e instanceof UnresolvedTokensError)) throw e; e.tokens.forEach((t) => missing.add(t)); return value; }
    }
    if (Array.isArray(value)) return value.map(walk);
    if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, walk(v)]));
    return value;
  };
  const result = { ...journey };
  for (const key of ['title', 'steps', 'persona', 'goal', 'success', 'run'] as const)
    if (key in journey) Object.assign(result, { [key]: walk(journey[key]) });
  if (missing.size) throw new UnresolvedTokensError([...missing]);
  return result;
}
