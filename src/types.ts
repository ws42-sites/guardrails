export type Mode = 'block' | 'warn' | 'off';
export type Tier = 'T1' | 'T2' | 'T3';

export interface Violation {
  rule: string;
  file: string;
  line?: number;
  message: string;
  /** Stable key for baselining: never includes a line number. */
  fingerprint: string;
  /** Cap the effective mode (e.g. T2 coverage gaps only warn). */
  maxMode?: Mode;
}

export interface Finding extends Violation {
  mode: Mode;
  known: boolean;
}

export interface CheckContext {
  cwd: string;
  config: import('./config.js').GuardrailsConfig;
  /** Git ref to diff against (merge-base is taken against HEAD). Null = full-repo mode. */
  base: string | null;
  labels: string[];
  options: { clockShift: boolean; mutation: boolean; knip: boolean; mutate?: string[]; failsOnBase?: boolean; specs?: string[] };
}

export interface Metric {
  name: string;
  value: number | string;
  detail?: string;
}

export interface JourneyRow {
  id: string;
  title: string;
  priority: 'P0' | 'P1' | 'P2';
  approved: boolean;
  specs: { id: string; tests: number }[];
  e2e: string[];
  /** draft = legacy unapproved; inferred = unapproved producer journey (warn only); missing = no e2e test; covered = test exists, no results; pass/fail from resultsFile. */
  status: 'inferred' | 'draft' | 'missing' | 'covered' | 'pass' | 'fail';
}

export interface CheckOutput {
  violations: Violation[];
  metrics?: Metric[];
  journeys?: JourneyRow[];
}

export interface Report {
  ok: boolean;
  base: string | null;
  findings: Finding[];
  metrics: Metric[];
  counts: { block: number; warn: number; known: number };
  staleBaseline: number;
  journeys?: JourneyRow[];
}
