import type { PageLike, RouteLike } from '../../src/cuj-runtime.js';

export function fakePage() {
  const calls: unknown[][] = [];
  const routes: { match: string | ((url: URL) => boolean); handler: (route: RouteLike) => Promise<void> }[] = [];
  const state = { path: '/', href: 'tel:555', scroll: true, fail: '', method: '', requestUrl: 'https://site.test/api/send' };
  const record = (name: string, ...args: unknown[]) => { calls.push([name, ...args]); if (state.fail === name) throw new Error('broken ' + name); };
  const locator = (name: string, ...args: unknown[]) => {
    record(name, ...args);
    const loc = {
      async click(options: unknown) { record('click', options); await request(); },
      async fill(value: string, options: unknown) { record('fill', value, options); },
      async waitFor(options: unknown) { record('waitFor', options); },
      first() { record('first'); return loc; },
      async getAttribute(attr: string, options: unknown) { record('getAttribute', attr, options); return state.href; },
      async evaluate(fn: (el: HTMLElement) => void, arg: undefined, options: unknown) {
        record('submit', arg, options); fn({ requestSubmit: () => record('requestSubmit') } as unknown as HTMLElement); await request();
      },
    };
    return loc;
  };
  async function request() {
    if (!state.method) return;
    const route: RouteLike = {
      request: () => ({ method: () => state.method, url: () => state.requestUrl }),
      async abort() { record('abort'); }, async continue() { record('continue'); },
      async fulfill(options) { record('fulfill', options); },
    };
    const entry = [...routes].reverse().find((r) => typeof r.match === 'string' || r.match(new URL(state.requestUrl)));
    await entry?.handler(route);
  }
  const page: PageLike = {
    async goto(path, options) { record('goto', path, options); state.path = path; await request(); },
    url() { record('url'); return 'https://site.test' + state.path; },
    getByRole: (role, options) => locator('role', role, options),
    getByText: (text) => locator('text', text), getByLabel: (label) => locator('label', label),
    locator: (selector) => locator('locator', selector),
    async route(match, handler) { record('route', typeof match === 'string' ? match : 'predicate'); routes.push({ match, handler }); },
    async setViewportSize(size) { record('viewport', size); },
    async evaluate<T>(fn: () => T) {
      record('evaluate');
      const previous = Object.getOwnPropertyDescriptor(globalThis, 'document');
      Object.defineProperty(globalThis, 'document', { configurable: true, value: { documentElement: { scrollWidth: state.scroll ? 390 : 400, clientWidth: 390 } } });
      try { return fn(); } finally { if (previous) Object.defineProperty(globalThis, 'document', previous); else Reflect.deleteProperty(globalThis, 'document'); }
    },
  };
  return { page, state, calls, request };
}
