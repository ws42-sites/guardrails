import { expect, it, vi } from 'vitest';
import { runJourney, type Step } from '../src/cuj-runtime.js';
import { fakePage } from './helpers/cuj-page.js';

const journey = (run: Step[]) => ({ id: 'CUJ-CONTACT', title: 'Visitor makes contact', run });
it('CUJ-07 executes every step in order with default timeout and literal case-insensitive text', async () => {
  const f = fakePage();
  await runJourney(f.page, journey([
    { goto: '/contact' }, { click: { role: 'button', name: 'Contact' } }, { click: { text: 'Open' } },
    { fill: { label: 'Name', value: 'Josh' } }, { submit: 'form' }, { expectText: 'Thanks (Josh)' },
    { expectUrl: '/contact' }, { expectVisible: { text: 'Sent' } }, { expectVisible: { role: 'link', name: 'Call' } },
    { expectHref: { selector: 'a', value: 'tel:555' } }, { expectNoHorizontalScroll: true },
    { viewport: { width: 390, height: 844 } }, { mockRoute: { match: '/api', status: 201 } },
  ]));
  expect(f.calls).toEqual([
    ['route', '**/*'], ['goto', '/contact', { timeout: 10000 }], ['role', 'button', { name: 'Contact' }], ['click', { timeout: 10000 }],
    ['text', 'Open'], ['click', { timeout: 10000 }], ['label', 'Name'], ['fill', 'Josh', { timeout: 10000 }],
    ['locator', 'form'], ['submit', undefined, { timeout: 10000 }], ['requestSubmit'], ['text', /Thanks \(Josh\)/i], ['first'],
    ['waitFor', { state: 'visible', timeout: 10000 }], ['url'], ['text', 'Sent'], ['first'], ['waitFor', { state: 'visible', timeout: 10000 }],
    ['role', 'link', { name: 'Call' }], ['first'], ['waitFor', { state: 'visible', timeout: 10000 }], ['locator', 'a'], ['first'],
    ['getAttribute', 'href', { timeout: 10000 }], ['evaluate'], ['viewport', { width: 390, height: 844 }], ['route', 'predicate'],
  ]);
});
it.each<[Step, string]>([
  [{ goto: '/' }, 'goto'], [{ click: { text: 'Go' } }, 'click'], [{ fill: { label: 'Name', value: 'Josh' } }, 'fill'],
  [{ submit: 'form' }, 'submit'], [{ expectText: 'Hello' }, 'waitFor'], [{ expectUrl: '/' }, 'url'],
  [{ expectVisible: { text: 'Hello' } }, 'waitFor'], [{ expectHref: { selector: 'a', value: 'tel:555' } }, 'getAttribute'],
  [{ expectNoHorizontalScroll: true }, 'evaluate'], [{ viewport: { width: 390, height: 844 } }, 'viewport'], [{ mockRoute: { match: '/api', status: 200 } }, 'route'],
])('CUJ-07 contextualises step failure %j', async (step, failure) => {
  const f = fakePage();
  // Route failure for mockRoute occurs after successful guard installation.
  const route = f.page.route;
  let installed = false;
  f.page.route = async (...args) => { if (installed && failure === 'route') throw new Error('broken route'); await route(...args); installed = true; };
  if (failure !== 'route') f.state.fail = failure;
  await expect(runJourney(f.page, journey([step]))).rejects.toThrow(`CUJ CUJ-CONTACT step 1 (${Object.keys(step)[0]}): broken ${failure}`);
});
it.each<[Step, string]>([
  [{ expectUrl: '/missing' }, 'expected path /missing, got /'],
  [{ expectHref: { selector: 'a', value: '/wrong' } }, 'expected href /wrong, got tel:555'],
  [{ expectNoHorizontalScroll: true }, 'horizontal scroll detected'],
  [{ goto: '//outside.test' }, 'goto must be a relative site path'],
])('CUJ-07 rejects incorrect expectations %j', async (step, reason) => {
  const f = fakePage(); f.state.scroll = false;
  await expect(runJourney(f.page, journey([{ goto: '/' }, step]))).rejects.toThrow(`CUJ CUJ-CONTACT step 2 (${Object.keys(step)[0]}): ${reason}`);
});
it('CUJ-07 bounds hanging operations with the configured timeout', async () => {
  vi.useFakeTimers();
  try {
    const f = fakePage(); f.page.goto = () => new Promise(() => {});
    const result = expect(runJourney(f.page, journey([{ goto: '/' }]), { stepTimeoutMs: 25 })).rejects.toThrow('CUJ CUJ-CONTACT step 1 (goto): timed out after 25 ms');
    await vi.advanceTimersByTimeAsync(25); await result;
  } finally { vi.useRealTimers(); }
});
it.each(['POST', 'PUT', 'PATCH', 'DELETE'])('CUJ-08 aborts unmocked %s without leaking query strings', async (method) => {
  const f = fakePage(); f.state.method = method; f.state.requestUrl += '?private=value';
  await expect(runJourney(f.page, journey([{ goto: '/' }, { click: { text: 'never' } }]))).rejects.toThrow(`CUJ CUJ-CONTACT step 1 (goto): unmocked write request: ${method} site.test/api/send`);
  expect(f.calls).toEqual([['route', '**/*'], ['goto', '/', { timeout: 10000 }], ['abort']]);
});
it.each(['GET', 'HEAD', 'POST'])('CUJ-08 permits safe %s and fulfils declared mocks even before the mock step', async (method) => {
  const f = fakePage(); f.state.method = method;
  const mocks: Step[] = method === 'POST' ? [{ mockRoute: { match: '/api', status: 202 } }] : [];
  await runJourney(f.page, journey([{ goto: '/' }, ...mocks, { click: { text: 'send' } }]));
  expect(f.calls.filter(([name]) => ['continue', 'fulfill', 'abort'].includes(String(name)))).toEqual(method === 'POST'
    ? [['fulfill', { status: 202, contentType: 'application/json', body: '{}' }], ['fulfill', { status: 202, contentType: 'application/json', body: '{}' }]]
    : [['continue'], ['continue']]);
});
