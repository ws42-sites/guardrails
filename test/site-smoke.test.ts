import { expect, it } from 'vitest';
import { smoke } from '../src/runbook-cmds.js';

function responses(pages: Record<string, string | number>) {
  const calls: string[] = [];
  const request: typeof fetch = async (input) => {
    const url = String(input);
    calls.push(url);
    const page = pages[new URL(url).pathname] ?? 404;
    return new Response(typeof page === 'string' ? page : '', { status: typeof page === 'number' ? page : 200 });
  };
  return { calls, request };
}

it('SITE-04 follows Astro sitemap indexes and rewrites only canonical-origin paths to the preview', async () => {
  const f = responses({
    '/sitemap-index.xml': '<sitemapindex><sitemap><loc>https://client.example/sitemap-0.xml</loc></sitemap><sitemap><loc>https://other.example/ignored.xml</loc></sitemap></sitemapindex>',
    '/sitemap-0.xml': '<urlset><url><loc>https://client.example/</loc></url><url><loc>https://client.example/about?q=1&amp;x=2</loc></url><url><loc>https://other.example/ignored</loc></url><url><loc>https://client.example/about?q=1&amp;x=2</loc></url></urlset>',
    '/': 200, '/about': 200, '/explicit': 200,
  });
  const result = await smoke('http://localhost:4321', ['/explicit'], 1000, { sitemap: true, fetch: f.request });
  expect(result.ok).toBe(true);
  expect(result.lines).toHaveLength(3);
  expect(f.calls).toEqual(['http://localhost:4321/sitemap-index.xml', 'http://localhost:4321/sitemap-0.xml', 'http://localhost:4321/explicit', 'http://localhost:4321/', 'http://localhost:4321/about?q=1&x=2']);
});

it('SITE-04 falls back to sitemap.xml and fails when a discovered page is broken', async () => {
  const f = responses({ '/sitemap.xml': '<urlset><url><loc>https://client.example/missing</loc></url></urlset>', '/': 200 });
  const result = await smoke('http://localhost:4321', [], 1000, { sitemap: true, fetch: f.request });
  expect(result.ok).toBe(false);
  expect(result.lines[1]).toContain('FAIL 404 http://localhost:4321/missing');
  expect(f.calls).toEqual(['http://localhost:4321/sitemap-index.xml', 'http://localhost:4321/sitemap.xml', 'http://localhost:4321/', 'http://localhost:4321/missing']);
});

it.each([
  [{}, 'neither /sitemap-index.xml nor /sitemap.xml is available'],
  [{ '/sitemap-index.xml': '<html>Not a sitemap</html>' }, 'invalid sitemap XML'],
  [{ '/sitemap-index.xml': '<urlset></urlset>' }, 'sitemap contains no same-origin page URLs'],
  [{ '/sitemap-index.xml': '<sitemapindex><loc>https://client.example/bad.xml</loc></sitemapindex>' }, '/bad.xml returned 404'],
])('SITE-04 reports sitemap discovery failure: %j', async (pages, message) => {
  const f = responses({ ...pages, '/': 200 });
  const result = await smoke('http://localhost:4321', [], 1000, { sitemap: true, fetch: f.request });
  expect(result.ok).toBe(false);
  expect(result.lines[0]).toBe(`FAIL sitemap: ${message}`);
  expect(result.lines[1]).toContain('ok   200 http://localhost:4321/');
});

it('SITE-04 handles cyclic indexes without repeating downloads and keeps sitemap discovery opt-in', async () => {
  const f = responses({
    '/sitemap-index.xml': '<sitemapindex><loc>https://client.example/nested.xml</loc></sitemapindex>',
    '/nested.xml': '<sitemapindex><loc>https://client.example/nested.xml</loc><loc>https://client.example/pages.xml</loc></sitemapindex>',
    '/pages.xml': '<urlset><loc>https://client.example/</loc></urlset>', '/': 200,
  });
  expect((await smoke('http://localhost:4321', [], 1000, { sitemap: true, fetch: f.request })).ok).toBe(true);
  expect(f.calls).toHaveLength(4);
  f.calls.length = 0;
  expect((await smoke('http://localhost:4321', [], 1000, { fetch: f.request })).ok).toBe(true);
  expect(f.calls).toEqual(['http://localhost:4321/']);
});
