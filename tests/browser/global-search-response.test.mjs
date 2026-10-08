import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { captureSearchResponses } from './helpers/global-search-response.mjs';

test('search capture survives actual client consumption and controller cleanup with one GET', { timeout: 30000 }, async () => {
  const bundle = await readFile(new URL('../../apps/public/static/global-search.v1.js', import.meta.url));
  const requests = [];
  const server = createServer((request, response) => {
    request.resume();
    if (request.url.startsWith('/search/api?')) {
      requests.push(request.url);
      response.writeHead(200, { 'content-type': 'application/json' });
      response.write('{"threads":[],');
      response.end('"offset":10,"nhits":0}');
    } else if (request.url === '/static/global-search.v1.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(bundle);
    } else response.writeHead(200, { 'content-type': 'text/html' }).end('<!doctype html><title>Search capture</title>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch({ timeout: 15000 });
    const page = await browser.newPage();
    await page.addInitScript(captureSearchResponses);
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const result = await page.evaluate(async () => {
      const { requestSearch } = await import('/static/global-search.v1.js');
      const controller = new AbortController();
      try { return await requestSearch({ query: 'owned', offset: 10, signal: controller.signal }); }
      finally { controller.abort(); }
    });
    assert.deepEqual(result, { threads: [], offset: 10, nhits: 0 });
    assert.deepEqual(await page.evaluate(() => Promise.all(window.ownedSearchResponses)), [{
      status: 200, type: 'application/json', result,
    }]);
    assert.deepEqual(requests, ['/search/api?q=owned&o=10']);
    await page.goto('about:blank');
    assert.equal(await page.evaluate(() => window.ownedSearchResponses.length), 0);
  } finally {
    if (browser) await browser.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});

test('bounded search observer retains the original body on invalid or oversized capture', async () => {
  const savedWindow = globalThis.window, savedLocation = globalThis.location;
  try {
    for (const bytes of [new Uint8Array([0xff]), new TextEncoder().encode('private invalid JSON'),
      new TextEncoder().encode(JSON.stringify({ text: 'x'.repeat(4096) }))]) {
      let calls = 0;
      const original = new Response(bytes, { headers: { 'content-type': 'application/json' } });
      globalThis.window = { fetch: async () => { calls++; return original; } };
      globalThis.location = { href: 'http://127.0.0.1/' };
      captureSearchResponses();
      const response = await window.fetch('/search/api?q=owned', { method: 'GET' });
      assert.equal(response, original);
      assert.deepEqual(new Uint8Array(await response.arrayBuffer()), bytes);
      assert.deepEqual(await Promise.all(window.ownedSearchResponses), [{ status: 200,
        type: 'application/json', failed: true }]);
      assert.equal(calls, 1);
    }
  } finally {
    if (savedWindow === undefined) delete globalThis.window; else globalThis.window = savedWindow;
    if (savedLocation === undefined) delete globalThis.location; else globalThis.location = savedLocation;
  }
});
