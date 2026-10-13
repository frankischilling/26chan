import './full-build.mjs';
import { createServer } from 'node:http';
import { readFile, readdir } from 'node:fs/promises';
import { makeHandler } from './serve.mjs';
const files = new Map();
for (const name of ['full-worker.mjs','full-cases.mjs','cases.mjs']) files.set(`/${name}`,await readFile(new URL(name,import.meta.url)));
for (const name of await readdir(new URL('./full-generated/',import.meta.url))) {
  if (/^[a-z.-]+\.js$/.test(name)) files.set(`/full-generated/${name}`,await readFile(new URL(`full-generated/${name}`,import.meta.url)));
}
files.set('/', Buffer.from('<!doctype html><html><head><meta charset="utf-8"><title>Full replay qualification</title></head><body></body></html>'));
createServer((request,response)=>{
  // Existing strict handler uses mjs MIME; fixed .js routes are JS too.
  if (request.url?.endsWith('.js')) {
    const bytes = files.get(request.url);
    if (!['GET','HEAD'].includes(request.method) || !bytes) {response.writeHead(404).end();return;}
    response.writeHead(200,{'Content-Type':'text/javascript','X-Content-Type-Options':'nosniff'}).end(request.method==='HEAD'?undefined:bytes);return;
  }
  makeHandler(files)(request,response);
}).listen(8790,'127.0.0.1');
