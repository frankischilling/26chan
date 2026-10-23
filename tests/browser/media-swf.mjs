import assert from 'node:assert/strict';
import http from 'node:http';
import { chromium } from '@playwright/test';

const [parent, player, download] = process.argv.slice(2).map(value => new URL(value));
for (const url of [parent, player, download]) assert.equal(url.hostname, '127.0.0.1');
assert.match(player.pathname, /^\/player\/f\/[1-9][0-9]*$/);
const movie = new URL(player.href.replace('/player/', '/player-data/'));
let canaryHits = 0;
const canary = http.createServer((_request, response) => {
  canaryHits++;
  response.writeHead(200, { 'Access-Control-Allow-Origin': '*', 'Content-Type': 'text/plain' });
  response.end('owned canary');
});
await new Promise(resolve => canary.listen(0, '127.0.0.1', resolve));
const canaryUrl = `http://127.0.0.1:${canary.address().port}/owned-network-probe`;
let browser;
try {
  browser = await chromium.launch({ timeout: 15000 });
  const context = await browser.newContext({ viewport: { width: 900, height: 700 } });
  await context.addCookies([{ name: 'owned_private_cookie', value: 'not-for-the-player', url: parent.origin }]);
  const page = await context.newPage();
  page.setDefaultTimeout(20000);
  const failures = [], credentials = [];
  let wasm = 0, movies = 0;
  page.on('pageerror', error => failures.push(error.message));
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.origin === player.origin && (url.pathname.startsWith('/player-assets/') || url.pathname.startsWith('/player-data/'))) {
      credentials.push(request.allHeaders().then(headers => assert.equal(headers.cookie, undefined, 'opaque-frame subresources must omit cookies')));
      if (url.pathname.endsWith('.wasm')) wasm++;
      if (url.pathname.startsWith('/player-data/')) { assert.equal(url.href, movie.href); movies++; }
    }
  });
  const documentResponse = await page.request.get(player.href);
  assert.equal(documentResponse.status(), 200);
  const csp = documentResponse.headers()['content-security-policy'];
  assert.ok(csp.includes('sandbox allow-scripts;'));
  assert.ok(!csp.includes('allow-same-origin'));
  assert.ok(csp.includes(`frame-ancestors ${parent.origin}`));
  assert.equal(documentResponse.headers()['x-frame-options'], undefined);
  assert.equal(documentResponse.headers()['set-cookie'], undefined);
  await page.goto(parent.href);
  const handle = await page.locator('iframe').elementHandle();
  const frame = await handle.contentFrame();
  assert.equal(frame.url(), player.href);
  await frame.waitForFunction(() => document.getElementById('status')?.hidden === true);
  await frame.locator('ruffle-player').click({ position: { x: 320, y: 240 } });
  const isolation = await frame.evaluate(() => {
    const denied = action => { try { action(); return false; } catch { return true; } };
    return { origin: window.origin, cookie: denied(() => document.cookie), storage: denied(() => localStorage.length),
      parent: denied(() => window.parent.document.body), script: window.RufflePlayer.config.allowScriptAccess,
      links: window.RufflePlayer.config.openUrlMode, source: document.getElementById('player').dataset.movie };
  });
  assert.deepEqual(isolation, { origin: 'null', cookie: true, storage: true, parent: true, script: false, links: 'deny', source: movie.href });
  const decoder = await context.newPage();
  let pixel;
  const deadline = Date.now() + 15000;
  do {
    await page.bringToFront();
    const screenshot = await page.locator('iframe').screenshot();
    pixel = await decoder.evaluate(async encoded => {
      const bytes = Uint8Array.from(atob(encoded), value => value.charCodeAt(0));
      const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
      const canvas = document.createElement('canvas'); canvas.width = bitmap.width; canvas.height = bitmap.height;
      const context = canvas.getContext('2d'); context.drawImage(bitmap, 0, 0); bitmap.close();
      return Array.from(context.getImageData(Math.floor(canvas.width / 2), Math.floor(canvas.height / 2), 1, 1).data);
    }, screenshot.toString('base64'));
    if ([18, 52, 86, 255].every((value, index) => Math.abs(pixel[index] - value) <= 2)) break;
    await page.waitForTimeout(100);
  } while (Date.now() < deadline);
  assert.ok([18, 52, 86, 255].every((value, index) => Math.abs(pixel[index] - value) <= 2), `actual movie background was not rendered: ${pixel}`);
  assert.ok(wasm >= 1, 'the pinned WebAssembly runtime must actually load');
  assert.equal(movies, 1);
  await Promise.all(credentials);
  assert.deepEqual(failures, []);
  const original = await page.request.get(download.href);
  assert.equal(original.status(), 200);
  assert.match(original.headers()['content-disposition'], /^attachment;.*filename\*=UTF-8''/);
  assert.ok(['FWS', 'CWS'].includes((await original.body()).subarray(0, 3).toString()));
  const data = await page.request.get(movie.href, { headers: { Origin: 'null' } });
  assert.equal(data.status(), 200);
  assert.equal(data.headers()['access-control-allow-origin'], '*');
  assert.equal(data.headers()['access-control-allow-credentials'], undefined);
  assert.deepEqual(await data.body(), await original.body());
  assert.equal((await page.request.get(canaryUrl)).status(), 200); canaryHits = 0;
  const blocked = await frame.evaluate(async url => { try { await fetch(url); return false; } catch { return true; } }, canaryUrl);
  assert.equal(blocked, true);
  assert.equal(canaryHits, 0, 'CSP must stop the request before any canary bytes are sent');
  const topBlocked = await frame.evaluate(url => { try { window.top.location.href = url; return false; } catch { return true; } }, canaryUrl);
  assert.equal(topBlocked, true);
  assert.equal(page.url(), parent.href);
  assert.equal(canaryHits, 0);
  console.log('PASS real FWS/CWS playback, original-byte downloads, opaque storage boundary, credentialless reads and blocked network/navigation probes');
} finally {
  if (browser) await browser.close();
  await new Promise(resolve => canary.close(resolve));
}
