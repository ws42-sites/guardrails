/** Portable runtime, copied into generated sites at build time. */
export type LocatorSpec = { role?: string; name?: string; text?: string };
export type Step =
  | { goto: string } | { click: LocatorSpec } | { fill: { label: string; value: string } }
  | { submit: string } | { expectText: string } | { expectUrl: string }
  | { expectVisible: LocatorSpec } | { expectHref: { selector: string; value: string } }
  | { expectNoHorizontalScroll: true } | { viewport: { width: number; height: number } }
  | { mockRoute: { match: string; status: number } };
interface LocatorLike {
  click(options: { timeout: number }): Promise<unknown>;
  fill(value: string, options: { timeout: number }): Promise<unknown>;
  waitFor(options: { state: 'visible'; timeout: number }): Promise<unknown>;
  first(): LocatorLike;
  getAttribute(name: string, options: { timeout: number }): Promise<string | null>;
  evaluate(fn: (element: HTMLElement) => void, arg: undefined, options: { timeout: number }): Promise<unknown>;
}
export interface RouteLike {
  request(): { method(): string; url(): string };
  abort(): Promise<unknown>;
  continue(): Promise<unknown>;
  fulfill(options: { status: number; contentType: string; body: string }): Promise<unknown>;
}
export interface PageLike {
  goto(url: string, options: { timeout: number }): Promise<unknown>;
  url(): string;
  getByRole(role: never, options: { name: string }): LocatorLike;
  getByText(text: string | RegExp): LocatorLike;
  getByLabel(label: string): LocatorLike;
  locator(selector: string): LocatorLike;
  route(url: string | ((url: URL) => boolean), handler: (route: RouteLike) => Promise<void>): Promise<unknown>;
  setViewportSize(size: { width: number; height: number }): Promise<unknown>;
  evaluate<T>(fn: () => T): Promise<T>;
}

export async function runJourney(page: PageLike, journey: { id: string; title: string; run: Step[] }, opts: { stepTimeoutMs?: number } = {}): Promise<void> {
  const timeout = opts.stepTimeoutMs ?? 10000;
  const mocks = journey.run.flatMap((s) => 'mockRoute' in s ? [s.mockRoute] : []);
  let writeError: Error | undefined;
  let failWrite: (error: Error) => void = () => {};
  const blocked = new Promise<never>((_resolve, reject) => { failWrite = reject; });
  // Observe writes between awaited steps without an unhandled rejection.
  void blocked.catch(() => {});
  const locate = (s: LocatorSpec) => s.text ? page.getByText(s.text) : page.getByRole(s.role as never, { name: s.name! });
  let index = 0;
  try {
    await page.route('**/*', async (route) => {
      const request = route.request();
      if (!['GET', 'HEAD'].includes(request.method()) && !mocks.some((m) => request.url().includes(m.match))) {
        const url = new URL(request.url());
        writeError = new Error(`unmocked write request: ${request.method()} ${url.host}${url.pathname}`);
        failWrite(writeError);
        await route.abort();
      } else {
        // Declared mocks are safe even before their DSL step is reached.
        const mock = mocks.find((m) => request.url().includes(m.match));
        if (mock) await route.fulfill({ status: mock.status, contentType: 'application/json', body: '{}' });
        else await route.continue();
      }
    });
    for (index = 0; index < journey.run.length; index++) {
      const step = journey.run[index];
      let timer: ReturnType<typeof setTimeout> | undefined;
      const execute = async () => {
        if ('goto' in step) {
          if (!step.goto.startsWith('/') || step.goto.startsWith('//')) throw new Error('goto must be a relative site path');
          await page.goto(step.goto, { timeout });
        } else if ('click' in step) await locate(step.click).click({ timeout });
        else if ('fill' in step) await page.getByLabel(step.fill.label).fill(step.fill.value, { timeout });
        else if ('submit' in step) await page.locator(step.submit).evaluate((el) => (el as HTMLFormElement).requestSubmit(), undefined, { timeout });
        else if ('expectText' in step) await page.getByText(new RegExp(step.expectText.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i')).first().waitFor({ state: 'visible', timeout });
        else if ('expectUrl' in step) {
          const actual = new URL(page.url()).pathname;
          if (actual !== step.expectUrl) throw new Error(`expected path ${step.expectUrl}, got ${actual}`);
        } else if ('expectVisible' in step) await locate(step.expectVisible).first().waitFor({ state: 'visible', timeout });
        else if ('expectHref' in step) {
          const actual = await page.locator(step.expectHref.selector).first().getAttribute('href', { timeout });
          if (actual !== step.expectHref.value) throw new Error(`expected href ${step.expectHref.value}, got ${actual}`);
        } else if ('expectNoHorizontalScroll' in step) {
          if (!await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)) throw new Error('horizontal scroll detected');
        } else if ('viewport' in step) await page.setViewportSize(step.viewport);
        else if ('mockRoute' in step) await page.route((url) => url.href.includes(step.mockRoute.match), async (route) => {
          await route.fulfill({ status: step.mockRoute.status, contentType: 'application/json', body: '{}' });
        });
      };
      try {
        if (writeError) throw writeError;
        await Promise.race([execute(), blocked, new Promise<never>((_resolve, reject) => {
          timer = setTimeout(() => reject(new Error(`timed out after ${timeout} ms`)), timeout);
        })]);
        if (writeError) throw writeError;
      } finally { clearTimeout(timer); }
    }
  } catch (error) {
    const step = journey.run[Math.min(index, journey.run.length - 1)];
    throw new Error(`CUJ ${journey.id} step ${index + 1} (${step ? Object.keys(step)[0] : 'setup'}): ${writeError?.message ?? (error instanceof Error ? error.message : String(error))}`);
  }
}
