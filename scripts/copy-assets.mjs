import { copyFileSync, chmodSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
copyFileSync('src/clock-shift.mjs', 'dist/clock-shift.mjs');
for (const f of ['dist/cli.js', 'dist/mcp.js']) chmodSync(f, 0o755);

mkdirSync('template/site', { recursive: true });
writeFileSync('template/site/cuj-runtime.ts', readFileSync('src/cuj-runtime.ts', 'utf8').replace(/\r\n/g, '\n'));
