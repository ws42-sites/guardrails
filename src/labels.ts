export const LEGACY_LABELS = {
  testsRemoved: ['tests-removed-approved'],
  baseline: ['guardrails:rebaseline'],
  characterization: ['allow-characterization'],
  flags: ['flags-resolved'],
  journeys: ['guardrails:config-approved'],
} as const;

const warned = new Set<string>();

export function hasLabel(labels: readonly string[], current: string, legacy: readonly string[] = []): boolean {
  if (labels.includes(current)) return true;
  const old = legacy.find((l) => l !== current && labels.includes(l));
  if (!old) return false;
  if (!warned.has(`${old}>${current}`)) {
    warned.add(`${old}>${current}`);
    process.stderr.write(`guardrails: warning: the label "${old}" is deprecated for this check and will stop working in a future release. Rename it to "${current}".\n`);
  }
  return true;
}
