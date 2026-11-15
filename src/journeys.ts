import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { z } from 'zod';
import { CONFIG_FILE, loadConfig, type GuardrailsConfig } from './config.js';
import { listFiles, matchesAny, readText } from './util.js';
import { routeOf } from './checks/journeys.js';

export type JourneyDraft = { title: string; priority: 'P0' | 'P1' | 'P2'; steps: string[]; specs: string[]; routes: string[]; approved: false };
type Fetch = (url: string, init: { method: string; headers: Record<string, string>; body: string }) => Promise<{ ok: boolean; status: number; json(): Promise<unknown> }>;

const LIBRARY = join(dirname(fileURLToPath(import.meta.url)), '..', 'journeys', 'library');
const ROUTE_LINE = /(?:<Route\b[^>]*\bpath=["'`{]+|\bpath:\s*["'`]|\b(?:app|router)\.(?:get|post|put|patch|delete)\(\s*["'`])(\/[^"'`}]*)/g;
const MOCK_EXT = /\.(png|jpe?g|webp|svg|fig|pdf)$/i;

/** Pages, endpoints and router paths in the project. */
export function collectRoutes(cwd: string, cfg: GuardrailsConfig, files = listFiles(cwd)): string[] {
  const out = new Set<string>();
  for (const f of files) {
    if (/node_modules|\.test\.|\.spec\./.test(f)) continue;
    if (matchesAny(f, cfg.journeys.routeGlobs) && /\.(m|c)?[jt]sx?$|\.astro$/.test(f)) out.add(routeOf(f));
    if (/^(src|app|functions|server)\/.*\.(m|c)?[jt]sx?$/.test(f)) for (const m of readText(cwd, f).matchAll(ROUTE_LINE)) out.add(m[1]);
  }
  return [...out].sort();
}

export function draftContext(cwd: string, cfg: GuardrailsConfig, files = listFiles(cwd)) {
  const docs = files.filter((f) => /(^|\/)(README|BRIEF|PROPOSAL)[^/]*\.md$/i.test(f) || /^docs\/(proposal|brief|design)?.*\.md$/i.test(f)).slice(0, 12);
  let budget = 24_000;
  const text = docs.map((f) => { const t = readText(cwd, f).slice(0, Math.max(0, budget)); budget -= t.length; return `--- ${f}\n${t}`; }).join('\n');
  const mocks = files.filter((f) => MOCK_EXT.test(f) && /(design|mock|figma|wireframe)/i.test(f)).map((f) => f.split('/').pop() as string);
  return { docs, text, routes: collectRoutes(cwd, cfg, files), mocks, specs: Object.entries(cfg.specs.items).map(([id, s]) => `${id}: ${s.behaviour}`) };
}

const LlmOut = z.object({ journeys: z.array(z.object({ id: z.string(), title: z.string(), priority: z.enum(['P0', 'P1', 'P2']).catch('P1'), steps: z.array(z.string()).min(1), routes: z.array(z.string()).default([]), specs: z.array(z.string()).default([]) })) });

function prompt(ctx: ReturnType<typeof draftContext>, max: number): string {
  return [
    `Draft ${Math.min(5, max)}-${max} Critical User Journeys (CUJs, Google eval practice) for this product.`,
    'A CUJ is a complete task a real user does end to end, in plain language (e.g. "User signs in and changes display name, sees it everywhere").',
    'Return JSON {"journeys":[{"id":"CUJ-<AREA>","title":"...","priority":"P0|P1|P2","steps":["..."],"routes":["/path"],"specs":["SPEC-ID"]}]}.',
    'P0 = the business fails without it; P1 = important; P2 = nice to have. Use only routes and spec IDs from the lists below. AREA is 2-16 uppercase letters/digits.',
    `Routes:\n${ctx.routes.join('\n') || '(none found)'}`,
    `Spec IDs:\n${ctx.specs.join('\n') || '(none)'}`,
    `Mock/design files:\n${ctx.mocks.join('\n') || '(none)'}`,
    `Brief and proposal docs:\n${ctx.text || '(none)'}`,
  ].join('\n\n');
}

async function llm(text: string, cfg: GuardrailsConfig, env: NodeJS.ProcessEnv, fetchImpl: Fetch): Promise<{ raw: unknown; via: string } | null> {
  if (env.GEMINI_API_KEY) {
    const model = cfg.journeys.draft.geminiModel;
    const r = await fetchImpl(`https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent`, {
      method: 'POST', headers: { 'content-type': 'application/json', 'x-goog-api-key': env.GEMINI_API_KEY },
      body: JSON.stringify({ contents: [{ parts: [{ text }] }], generationConfig: { responseMimeType: 'application/json', temperature: 0.2 } }),
    });
    if (!r.ok) throw new Error(`Gemini ${model} returned ${r.status}`);
    const j = (await r.json()) as { candidates?: { content?: { parts?: { text?: string }[] } }[] };
    return { raw: JSON.parse(j.candidates?.[0]?.content?.parts?.[0]?.text ?? '{}'), via: `gemini:${model}` };
  }
  if (env.OPENAI_API_KEY) {
    const model = cfg.journeys.draft.openaiModel;
    const r = await fetchImpl('https://api.openai.com/v1/chat/completions', {
      method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${env.OPENAI_API_KEY}` },
      body: JSON.stringify({ model, temperature: 0.2, response_format: { type: 'json_object' }, messages: [{ role: 'user', content: text }] }),
    });
    if (!r.ok) throw new Error(`OpenAI ${model} returned ${r.status}`);
    const j = (await r.json()) as { choices?: { message?: { content?: string } }[] };
    return { raw: JSON.parse(j.choices?.[0]?.message?.content ?? '{}'), via: `openai:${model}` };
  }
  return null;
}

const P0_WORDS = /(sign|login|auth|checkout|pay|book|contact|order|review|submit|intake|support)/i;
/** No LLM: one journey per top-level route group. */
export function heuristicDrafts(routes: string[], max: number): Record<string, JourneyDraft> {
  const groups = new Map<string, string[]>();
  for (const r of routes) {
    const seg = r.split('/').filter(Boolean);
    const key = seg[0] === 'api' ? (seg[1] ?? 'api') : (seg[0] ?? 'home');
    groups.set(key, [...(groups.get(key) ?? []), r]);
  }
  const out: Record<string, JourneyDraft> = {};
  for (const [key, rs] of [...groups].slice(0, max)) {
    const area = key.replace(/[^a-z0-9]/gi, '').toUpperCase().slice(0, 16).padEnd(2, 'X');
    const page = rs.find((r) => !r.startsWith('/api')) ?? rs[0];
    out[`CUJ-${area}`] = {
      title: `User opens ${page} and completes the ${key} task end to end`,
      priority: P0_WORDS.test(key) ? 'P0' : 'P1',
      steps: [`Open ${page}`, `Complete the main ${key} action`, 'See the result confirmed', ...(rs.some((r) => r.startsWith('/api')) ? [`Data reaches ${rs.filter((r) => r.startsWith('/api')).join(', ')}`] : [])],
      specs: [], routes: rs, approved: false,
    };
  }
  return out;
}

export async function draftJourneys(cwd: string, opts: { env?: NodeJS.ProcessEnv; fetch?: Fetch } = {}): Promise<{ drafts: Record<string, JourneyDraft>; via: string; context: ReturnType<typeof draftContext> }> {
  const cfg = loadConfig(cwd);
  const env = opts.env ?? process.env;
  const ctx = draftContext(cwd, cfg);
  const max = cfg.journeys.draft.max;
  const res = await llm(prompt(ctx, max), cfg, env, opts.fetch ?? (globalThis.fetch as unknown as Fetch));
  if (!res) return { drafts: heuristicDrafts(ctx.routes, max), via: 'heuristic (no GEMINI_API_KEY or OPENAI_API_KEY)', context: ctx };
  const idRe = new RegExp(cfg.journeys.idPattern);
  const drafts: Record<string, JourneyDraft> = {};
  for (const j of LlmOut.parse(res.raw).journeys.slice(0, max)) {
    const id = idRe.test(j.id) ? j.id : `CUJ-${j.id.replace(/^CUJ-?/i, '').replace(/[^A-Z0-9]/gi, '').toUpperCase().slice(0, 16)}`;
    if (!idRe.test(id) || j.title.length < 10) continue;
    drafts[id] = { title: j.title, priority: j.priority, steps: j.steps, specs: j.specs.filter((s) => cfg.specs.items[s]), routes: j.routes, approved: false };
  }
  return { drafts, via: res.via, context: ctx };
}

function editConfig(cwd: string, edit: (raw: Record<string, any>) => void): void {
  const p = join(cwd, CONFIG_FILE);
  const raw = JSON.parse(readFileSync(p, 'utf8'));
  edit(raw);
  writeFileSync(p, `${JSON.stringify(raw, null, 2)}\n`);
  loadConfig(cwd); // validate; throws ConfigError if the edit broke it
}

/** Agents (GUARDRAILS_AGENT set by the hook settings) may not add or approve journeys. */
export function assertHuman(env: NodeJS.ProcessEnv, what: string): void {
  if (env.GUARDRAILS_AGENT) throw new Error(`${what} is for humans only (GUARDRAILS_AGENT=${env.GUARDRAILS_AGENT}). Ask the owner to run it.`);
}

/** Add drafts that don't exist yet; never touches existing journeys. Returns the IDs added. */
export function addDrafts(cwd: string, drafts: Record<string, Omit<JourneyDraft, 'approved'> & { approved?: boolean }>): string[] {
  const added: string[] = [];
  editConfig(cwd, (raw) => {
    raw.journeys ??= {};
    raw.journeys.items ??= {};
    for (const [id, d] of Object.entries(drafts))
      if (!raw.journeys.items[id]) { raw.journeys.items[id] = { ...d, approved: false }; added.push(id); }
  });
  return added;
}

export function approveJourney(cwd: string, id: string, opts: { by?: string; env?: NodeJS.ProcessEnv } = {}): string {
  const env = opts.env ?? process.env;
  assertHuman(env, 'guardrails journeys approve');
  let by = opts.by;
  if (!by) try { by = execFileSync('git', ['config', 'user.name'], { cwd, encoding: 'utf8' }).trim(); } catch { /* no git */ }
  if (!by) throw new Error('pass --by <name>: approvals record who approved');
  editConfig(cwd, (raw) => {
    const j = raw.journeys?.items?.[id];
    if (!j) throw new Error(`no journey ${id}`);
    j.approved = true;
    j.approvedBy = by;
    for (const s of j.specs ?? []) {
      const spec = raw.specs?.items?.[s];
      if (spec && !(spec.journeys ?? []).includes(id)) spec.journeys = [...(spec.journeys ?? []), id];
    }
  });
  return by;
}

export interface Preset { preset: string; journeys: (Omit<JourneyDraft, 'approved' | 'specs'> & { id: string; optional?: boolean; when?: string[] })[] }
export function loadPreset(name: string): Preset {
  const p = join(LIBRARY, `${name}.json`);
  if (!/^[a-z0-9-]+$/.test(name) || !existsSync(p)) throw new Error(`unknown preset ${name}`);
  return JSON.parse(readFileSync(p, 'utf8')) as Preset;
}
/** Core journeys always; optional ones only when the repo mentions them (e.g. a menu page). */
export function presetDrafts(name: string, files: string[]): Record<string, Omit<JourneyDraft, 'approved'>> {
  const out: Record<string, Omit<JourneyDraft, 'approved'>> = {};
  for (const j of loadPreset(name).journeys) {
    if (j.optional && !files.some((f) => (j.when ?? []).some((w) => f.toLowerCase().includes(w)))) continue;
    out[j.id] = { title: j.title, priority: j.priority, steps: j.steps, routes: j.routes, specs: [] };
  }
  return out;
}
