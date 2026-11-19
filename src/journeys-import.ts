import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { CONFIG_FILE, journeySchema, parseConfig } from './config.js';

/** Validate the whole batch and resulting config before returning a merged copy. */
export function mergeJourneys(raw: Record<string, any>, input: unknown): Record<string, any> {
  const cfg = parseConfig(raw);
  if (!Array.isArray(input)) throw new Error('journeys import requires a JSON array');
  const items = { ...raw.journeys?.items };
  const ids = new Set<string>();
  for (const entry of input) {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) throw new Error('invalid journey object');
    const { id, ...body } = entry;
    if (typeof id !== 'string' || !new RegExp(cfg.journeys.idPattern).test(id) || ids.has(id)) throw new Error('invalid or duplicate journey id');
    ids.add(id);
    if (body.approved === true || Object.hasOwn(body, 'approvedBy')) throw new Error(`${id}: imports cannot approve journeys or supply approvedBy`);
    if (!body.provenance || !body.source?.by) throw new Error(`${id}: provenance and source.by are required`);
    const journey = journeySchema.parse(body);
    if (!cfg.journeys.trustedProducers.includes(journey.source!.by)) throw new Error(`${id}: untrusted producer ${journey.source!.by}`);
    const next = { ...journey, approved: false };
    const existing = cfg.journeys.items[id];
    // Exact replays are safe even for client-provided journeys; mutations are not.
    if (existing && isDeepStrictEqual(existing, next)) continue;
    if (existing && (existing.approved || existing.provenance === 'client-provided')) throw new Error(`${id}: cannot overwrite an approved or client-provided journey`);
    items[id] = next;
  }
  const merged = { ...raw, journeys: { ...raw.journeys, items } };
  parseConfig(merged);
  return merged;
}

export function importJourneys(cwd: string, file: string): string[] {
  const path = join(cwd, CONFIG_FILE);
  const raw = JSON.parse(readFileSync(path, 'utf8'));
  const merged = mergeJourneys(raw, JSON.parse(readFileSync(file, 'utf8')));
  const changed = Object.keys(merged.journeys.items).filter((id) => !isDeepStrictEqual(raw.journeys?.items?.[id], merged.journeys.items[id]));
  if (changed.length) writeFileSync(path, `${JSON.stringify(merged, null, 2)}\n`);
  return changed;
}
