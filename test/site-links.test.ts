import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { expect, it } from 'vitest';
import { checkLinks } from '../src/links.js';
import { siteTree } from './helpers/site.js';

it('SITE-02 reports broken href and src with file and line', () => {
  const dir = siteTree({ 'index.html': '<h1>Home</h1>\n<a href="/lost?x=1#top">Lost</a>\n<img src="missing.png">\n<script src="missing.js"></script>', 'nested/index.html': '<a href="../gone">Gone</a>' });
  expect(checkLinks(dir)).toEqual({ ok: false, contactMissing: false, broken: [
    { file: 'index.html', line: 2, target: '/lost?x=1#top' },
    { file: 'index.html', line: 3, target: 'missing.png' },
    { file: 'index.html', line: 4, target: 'missing.js' },
    { file: 'nested/index.html', line: 1, target: '../gone' },
  ] });
});

it('SITE-02 resolves directory indexes, suffixes, queries, nested parents and encoded names', () => {
  const dir = siteTree({
    'index.html': '<a href="/about/">About</a><a href="team?sort=name#top">Team</a><img src="/assets/my%20logo.svg?v=2"><a href="/">Home</a>',
    'about/index.html': '<a href="../team">Team</a><img src=../assets/logo.svg><a href="?q=1">Here</a>',
    'team.html': '<a href="/about">About</a>', 'assets/logo.svg': '', 'assets/my logo.svg': '',
  });
  expect(checkLinks(dir)).toEqual({ ok: true, broken: [], contactMissing: false });
});

it('SITE-02 CLI reports broken links and distinguishes usage errors', () => {
  const dir = siteTree({ 'index.html': '<a href="/missing">Missing</a>' });
  const cli = (...args: string[]) => spawnSync(process.execPath, [resolve('dist/cli.js'), 'check-links', ...args], { encoding: 'utf8' });
  const broken = cli(dir);
  expect([broken.status, broken.stdout.trim()]).toEqual([1, 'index.html:1 -> /missing']);
  expect(cli().status).toBe(2);
  expect(cli(join(dir, 'absent')).status).toBe(2);
  expect(cli(dir, 'extra').status).toBe(2);
  expect(cli(siteTree({ 'index.html': '<h1>Home</h1>' }))).toMatchObject({ status: 0, stdout: '' });
});

it('SITE-03 ignores remote URLs, schemes, fragments and markup in comments or scripts', () => {
  const dir = siteTree({ 'index.html': `<a href="https://example.com/no"><a href='http://example.com/no'><img src="//cdn.example.com/no"><a href="#top"><a href="mailto:hello@example.com"><a href="tel:+15555550123"><a href="javascript:void(0)"><img src="data:image/png;base64,AAAA"><!-- <a href="missing"> --><script>const s = '<a href="missing">';</script>` });
  expect(checkLinks(dir, { requireContact: true })).toEqual({ ok: true, broken: [], contactMissing: false });
});

it.each(['tel:+15555550123', 'mailto:hello@example.com'])('SITE-03 accepts either home-page contact link: %s', (contact) => {
  expect(checkLinks(siteTree({ 'index.html': `<a href="${contact}">Contact</a>` }), { requireContact: true })).toEqual({ ok: true, broken: [], contactMissing: false });
});

it('SITE-03 requires contact on the home page only when requested, including through CLI', () => {
  const dir = siteTree({ 'index.html': '<!-- <a href="tel:1">Call</a> --><img src="mailto:x@example.com">', 'contact.html': '<a href="tel:1">Call</a>' });
  expect(checkLinks(dir).ok).toBe(true);
  expect(checkLinks(dir, { requireContact: true })).toEqual({ ok: false, broken: [], contactMissing: true });
  const result = spawnSync(process.execPath, [resolve('dist/cli.js'), 'check-links', dir, '--require-contact'], { encoding: 'utf8' });
  expect([result.status, result.stdout.trim()]).toEqual([1, 'index.html:1 -> missing tel: or mailto: contact link']);
  expect(checkLinks(siteTree(), { requireContact: true }).ok).toBe(false);
});
