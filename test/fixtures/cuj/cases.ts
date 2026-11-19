export const j = (fields: object = {}) => ({ title: 'Visitor contacts the business', priority: 'P0', steps: ['Open contact'], provenance: 'inferred', source: { by: 'director' }, run: [{ goto: '/contact' }], ...fields });
export const block = (item: unknown, trust = ['director']) => ({ trustedProducers: trust, items: item ? { 'CUJ-CONTACT': item } : {} });
export const validSteps = [
  { goto: '/contact' }, { click: { role: 'button', name: 'Send' } }, { click: { text: 'Call' } },
  { fill: { label: 'Phone', value: '{{client.phone}}' } }, { submit: 'form > button[type="submit"]' },
  { expectText: 'Thank you' }, { expectUrl: '/thanks' }, { expectVisible: { text: 'Sent' } },
  { expectHref: { selector: 'a[href^="tel:"]', value: 'tel:{{client.phone}}' } },
  { expectNoHorizontalScroll: true }, { viewport: { width: 200, height: 4000 } },
  { mockRoute: { match: '/api/contact', status: 599 } }, { goto: '{{client.path}}' },
];
export const invalidSteps = [
  { evaluate: 'alert(1)' }, { goto: '/ok', expectText: 'extra' }, { click: { selector: '#raw' } },
  { click: { role: 'button' } }, { expectVisible: { name: 'alone' } }, { fill: { label: 'X', value: '' } },
  { goto: 'https://example.com' }, { expectUrl: 'relative' }, { expectText: 'x'.repeat(301) },
  { expectText: ' ' }, { submit: 'a,button' }, { submit: 'a\\b' }, { submit: 'button;eval()' },
  { expectHref: { selector: 'a+div', value: '/ok' } }, { expectNoHorizontalScroll: false },
  { viewport: { width: 199, height: 400 } }, { viewport: { width: 400, height: 4001 } },
  { viewport: { width: 200.5, height: 400 } }, { mockRoute: { match: '/api', status: 99 } },
  { mockRoute: { match: '/api', status: 600 } }, { mockRoute: { match: '/api', status: 200.5 } },
  { fill: { label: 'X', value: 'Y', js: 'bad' } },
];
export const invalidJourneyFields = [
  { source: { by: 'x', extra: true } },
  { source: { by: '{{client.producer}}' } }, { provenance: 'unknown' }, { persona: '' }, { goal: 'x'.repeat(301) },
  { surprise: true },
];
export const ownershipCases = [
  ['inferred addition', undefined, j(), false],
  ['client addition', undefined, j({ provenance: 'client-provided' }), false],
  ['template addition', undefined, j({ provenance: 'template', approved: false }), false],
  ['untrusted addition', undefined, j({ source: { by: 'stranger' } }), true],
  ['unsourced addition', undefined, j({ source: undefined }), true],
  ['missing provenance addition', undefined, j({ provenance: undefined }), true],
  ['missing run addition', undefined, j({ run: undefined }), true],
  ['invalid run addition', undefined, j({ run: [{ eval: 'bad' }] }), true],
  ['approved addition', undefined, j({ approved: true, approvedBy: 'Josh' }), true],
  ['trusted regeneration', j(), j({ goal: 'Updated goal' }), false],
  ['trusted removal', j(), undefined, false],
  ['template removal', j({ provenance: 'template' }), undefined, false],
  ['untrusted regeneration', j({ source: { by: 'stranger' } }), j(), true],
  ['untrusted removal', j({ source: { by: 'stranger' } }), undefined, true],
  ['approved edit', j({ approved: true, approvedBy: 'Josh' }), j({ goal: 'New' }), true],
  ['approved removal', j({ approved: true, approvedBy: 'Josh' }), undefined, true],
  ['client edit', j({ provenance: 'client-provided' }), j({ goal: 'New' }), true],
  ['client removal', j({ provenance: 'client-provided' }), undefined, true],
  ['approval flip', j(), j({ approved: true, approvedBy: 'Josh' }), true],
  ['client promotion', j(), j({ provenance: 'client-provided' }), true],
  ['producer substitution', j(), j({ source: { by: 'stranger' } }), true],
  ['legacy edit', j({ source: undefined, provenance: undefined }), j(), true],
  ['unchanged', j(), j(), false],
] as const;

const importedJourney = (fields: object = {}) => ({ id: 'CUJ-CONTACT', title: 'Visitor contacts the business', priority: 'P0', steps: ['Open contact'], provenance: 'inferred', source: { by: 'director' }, run: [{ goto: '/contact' }], ...fields });
export const cfg = { version: 1, journeys: { trustedProducers: ['director'], items: {} } };
export const importRefusals = [
  ...[ [importedJourney({ approved: true })], [importedJourney({ approvedBy: 'bot' })], [importedJourney({ provenance: undefined })],
    [importedJourney({ source: undefined })], [importedJourney({ source: { by: 'stranger' } })], [importedJourney({ run: [{ evaluate: 'bad' }] })],
    [importedJourney({ id: 'bad' })], [importedJourney(), importedJourney()], [importedJourney({ specs: ['UNKNOWN-01'] })], {},
  ].map((input) => ({ input, config: cfg, error: undefined })),
  ...[{ approved: true, approvedBy: 'Josh' }, { provenance: 'client-provided' }].map((fields) => {
    const { id, ...existing } = importedJourney(fields);
    return { input: [importedJourney({ id: 'CUJ-NEW' }), importedJourney({ goal: 'Changed' })], config: { ...cfg, journeys: { ...cfg.journeys, items: { [id]: existing } } }, error: 'cannot overwrite' };
  }),
];
