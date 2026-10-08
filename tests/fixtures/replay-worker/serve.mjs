// Owned, read-only static fixture server. No app, DB, media, arbitrary paths,
// script rewriting or external network routes. Used only by the dedicated config.
import { createServer } from 'node:http';
import { readFile, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { build } from './build.mjs';
export async function staticFiles() {
  await build(true);
  const allowed = ['index.html', 'reference.html', 'reference.mjs', 'main.mjs', 'cases.mjs',
    'load-case.mjs', 'comparison.mjs', 'semantic-checkpoints.mjs', 'raw-worker-protocol.mjs', 'worker-boundary.mjs', 'protocol.mjs', 'worker.mjs', 'controller.mjs', 'fault-worker.mjs'];
  for (const name of await readdir(new URL('./generated/', import.meta.url))) {
    if (/^(?:[a-z0-9.-]+\.(?:mjs|ibr|woff)|manifest\.json|tegaki\.css)$/.test(name)) allowed.push(`generated/${name}`);
  }
  const files = new Map();
  for (const path of allowed) files.set(`/${path}`, await readFile(new URL(path, import.meta.url)));
  files.set('/', files.get('/index.html')); return files;
}
export function makeHandler(files) {
  return (request, response) => {
    let path;
    try { path = new URL(request.url, 'http://fixture.invalid').pathname; } catch { response.writeHead(400).end(); return; }
    const bytes = files.get(path);
    if (!['GET', 'HEAD'].includes(request.method) || !bytes) { response.writeHead(404).end(); return; }
    const ext = path.split('.').pop();
    const mime = { html: 'text/html', mjs: 'text/javascript', json: 'application/json', css: 'text/css', woff: 'font/woff', ibr: 'application/octet-stream' }[ext] || 'text/html';
    response.writeHead(200, { 'Content-Type': `${mime}; charset=utf-8`, 'Content-Length': bytes.length,
      'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' });
    response.end(request.method === 'HEAD' ? undefined : bytes);
  };
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv.length > 2) throw new Error('serve.mjs takes no arguments');
  const port = Number(process.env.REPLAY_PROBE_PORT || 8789);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error('invalid fixture port');
  createServer(makeHandler(await staticFiles())).listen(port, '127.0.0.1', () => console.log(`Owned replay fixture on http://127.0.0.1:${port}`));
}
