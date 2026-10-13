import './full-build.mjs';
import '../../../scripts/build-native-replay.mjs';
import { createServer } from 'node:http';
import { readFile, readdir } from 'node:fs/promises';
import { makeHandler } from './serve.mjs';
const files = new Map();
for (const name of ['full-worker.mjs','full-cases.mjs','cases.mjs']) files.set(`/${name}`,await readFile(new URL(name,import.meta.url)));
for (const name of await readdir(new URL('./full-generated/',import.meta.url))) {
  if (/^[a-z.-]+\.js$/.test(name)) files.set(`/full-generated/${name}`,await readFile(new URL(`full-generated/${name}`,import.meta.url)));
}
for (const name of ['worker.v1.js','runtime.v1.js','player.v1.js','player.v1.css']) files.set(`/static/native-replay/${name}`, await readFile(new URL(`../../../apps/public/static/native-replay/${name}`,import.meta.url)));
files.set('/native-replay-presentation.js', await readFile(new URL('../../../apps/public/client/native-replay-presentation.js',import.meta.url)));
for (const name of ['tegaki-0.9.4.v1.css','tegaki-icons.v1.woff']) files.set(`/static/tegaki/${name}`, await readFile(new URL(`../../../apps/public/static/tegaki/${name}`,import.meta.url)));
files.set('/session.js', await readFile(new URL('../../../apps/public/client/native-replay-session.js',import.meta.url)));
const { encodeOwnedCase } = await import('./cases.mjs');
const { FULL_CASES } = await import('./full-cases.mjs');
const replay = encodeOwnedCase(FULL_CASES[0]);
new DataView(replay.buffer).setUint32(replay.length - 12, 60000);
files.set('/replay/i/123.ibr', Buffer.from(replay));
files.set('/player.html', Buffer.from(`<!doctype html><html><head><meta charset="utf-8"><title>Replay player qualification</title><link rel="stylesheet" href="/static/tegaki/tegaki-0.9.4.v1.css"><link rel="stylesheet" href="/static/native-replay/player.v1.css"></head><body><div id="watcher-context" hidden data-board="i" data-catalog="false" data-media-origin="http://127.0.0.1:8790"></div><article class="postContainer"><blockquote class="postMessage"><a href="#replay-123" data-drawing-replay="123">View</a><a href="#replay-124" data-drawing-replay="124">Missing</a></blockquote></article><script type="module" src="/static/native-replay/player.v1.js"></script></body></html>`));
files.set('/', Buffer.from('<!doctype html><html><head><meta charset="utf-8"><title>Full replay qualification</title></head><body></body></html>'));
createServer((request,response)=>{
  if (request.url === '/player.html' || request.url?.endsWith('.ibr') || request.url?.endsWith('.css') || request.url?.endsWith('.woff')) {
    const bytes = files.get(request.url);
    if (!['GET','HEAD'].includes(request.method) || !bytes) { response.writeHead(404).end(); return; }
    const type = request.url.endsWith('.html') ? 'text/html' : request.url.endsWith('.css') ? 'text/css' : request.url.endsWith('.woff') ? 'font/woff' : 'application/octet-stream';
    response.writeHead(200,{'Content-Type':type,'X-Content-Type-Options':'nosniff',
      'Content-Security-Policy': "default-src 'none'; script-src http://127.0.0.1:8790/static/native-replay/player.v1.js; worker-src http://127.0.0.1:8790/static/native-replay/worker.v1.js; connect-src http://127.0.0.1:8790/replay/i/; style-src 'self'; font-src 'self'"}).end(request.method==='HEAD'?undefined:bytes);return;
  }
  // Existing strict handler uses mjs MIME; fixed .js routes are JS too.
  if (request.url?.endsWith('.js')) {
    const bytes = files.get(request.url);
    if (!['GET','HEAD'].includes(request.method) || !bytes) {response.writeHead(404).end();return;}
    response.writeHead(200,{'Content-Type':'text/javascript','X-Content-Type-Options':'nosniff',...(request.url==='/static/native-replay/worker.v1.js'?{'Content-Security-Policy':"default-src 'none'; script-src http://127.0.0.1:8790/static/native-replay/runtime.v1.js; connect-src 'none'; worker-src 'none'"}:{})}).end(request.method==='HEAD'?undefined:bytes);return;
  }
  makeHandler(files)(request,response);
}).listen(8790,'127.0.0.1');
