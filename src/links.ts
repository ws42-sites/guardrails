import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, posix, resolve } from 'node:path';

export interface LinkProblem { file: string; line: number; target: string }
export interface LinksResult { ok: boolean; broken: LinkProblem[]; contactMissing: boolean }

/** Check a built tree without logging or exiting. Directory errors are usage errors for the caller. */
export function checkLinks(dir: string, options: { requireContact?: boolean } = {}): LinksResult {
  const root = resolve(dir);
  if (!statSync(root).isDirectory()) throw new Error(`check-links: not a directory: ${dir}`);
  const files = new Set<string>();
  const visit = (folder: string) => {
    for (const entry of readdirSync(join(root, folder), { withFileTypes: true })) {
      const path = posix.join(folder, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile()) files.add(path);
    }
  };
  visit('');
  const broken: LinkProblem[] = [];
  let contact = false;
  for (const file of [...files].filter((f) => /\.html$/i.test(f)).sort()) {
    // Mask non-markup while retaining offsets for useful line numbers.
    const html = readFileSync(join(root, file), 'utf8').replace(/<!--[\s\S]*?-->/g, (s) => s.replace(/[^\n]/g, ' '))
      .replace(/(<(script|style)\b[^>]*>)([\s\S]*?)(<\/\2\s*>)/gi, (_all, open, _tag, body: string, close) => open + body.replace(/[^\n]/g, ' ') + close);
    for (const tag of html.matchAll(/<[a-z](?:[^>"']|"[^"]*"|'[^']*')*>/gi)) {
      for (const attr of tag[0].matchAll(/\s(href|src)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/gi)) {
        const target = (attr[2] ?? attr[3] ?? attr[4]).trim();
        if (file === 'index.html' && attr[1].toLowerCase() === 'href' && /^<(?:a|area)\b/i.test(tag[0]) && /^(tel|mailto):/i.test(target)) contact = true;
        if (!target || /^(?:#|\/\/|[a-z][a-z\d+.-]*:)/i.test(target)) continue;
        let path: string;
        try {
          const pathname = decodeURIComponent(target.split(/[?#]/)[0]);
          path = pathname ? posix.normalize(pathname.startsWith('/') ? pathname.slice(1) : posix.join(posix.dirname(file), pathname)) : file;
        } catch { path = '../invalid-encoding'; }
        const candidates = [path, `${path.replace(/\/$/, '')}.html`, posix.join(path, 'index.html')];
        if (!candidates.some((p) => files.has(p))) {
          const offset = tag.index! + attr.index!;
          broken.push({ file, line: html.slice(0, offset).split('\n').length, target });
        }
      }
    }
  }
  const contactMissing = !!options.requireContact && !contact;
  return { ok: broken.length === 0 && !contactMissing, broken, contactMissing };
}
