import assert from 'node:assert/strict';
import { performance } from 'node:perf_hooks';
import { expect } from '@playwright/test';

export const tegakiModule = '/static/tegaki/tegaki-0.9.4.v1.js';
export const drawingControls = scope => ({
  draw: scope.locator('[data-drawing-draw]'), clear: scope.locator('[data-drawing-clear]'),
  width: scope.locator('[data-drawing-width]'), height: scope.locator('[data-drawing-height]'),
  status: scope.locator('[data-drawing-status]'),
});

export async function canvasProof(page) {
  return page.evaluate(async path => {
    const { Tegaki } = await import(path);
    const canvas = Tegaki.flatten(), pixels = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
    let ink = 0, white = 0;
    for (let offset = 0; offset < pixels.length; offset += 4) {
      if (pixels[offset + 3] && Math.min(pixels[offset], pixels[offset + 1], pixels[offset + 2]) < 240) ink++;
      if (pixels[offset] === 255 && pixels[offset + 1] === 255 && pixels[offset + 2] === 255 && pixels[offset + 3] === 255) white++;
    }
    const hash = [...new Uint8Array(await crypto.subtle.digest('SHA-256', pixels))].map(value => value.toString(16).padStart(2, '0')).join('');
    return { version: Tegaki.VERSION, width: canvas.width, height: canvas.height, ink, white, hash,
      replay: Tegaki.saveReplay, custom: Tegaki.hasCustomCanvas, layers: Tegaki.layers.length };
  }, tegakiModule);
}

export async function pointerDrawing(page) {
  await expect(page.locator('#tegaki-cursor-layer')).toBeVisible();
  const before = await canvasProof(page); assert.equal(before.version, '0.9.4'); assert.equal(before.ink, 0);
  // Use trusted mouse-generated pointer events on Tegaki's real stroke target.
  // No canvas draw calls, fixed PNG inputs, or synthetic replacement editor.
  const target = await page.locator('#tegaki-canvas').boundingBox(); assert.ok(target);
  await page.mouse.move(target.x + 55, target.y + 60); await page.mouse.down();
  await page.mouse.move(target.x + 165, target.y + 135, { steps: 35 });
  await page.mouse.move(target.x + 265, target.y + 75, { steps: 35 }); await page.mouse.up();
  await page.mouse.move(target.x + 100, target.y + 225); await page.mouse.down();
  await page.mouse.move(target.x + 260, target.y + 270, { steps: 35 }); await page.mouse.up();
  const proof = await canvasProof(page);
  assert.equal(proof.width, 400); assert.equal(proof.height, 400); assert.equal(proof.replay, false);
  assert.ok(proof.ink > 100, 'actual pointer strokes must export more than a blank canvas');
  assert.ok(proof.white > 10000, 'the white background remains visible'); assert.notEqual(proof.hash, before.hash);
  return proof;
}

export async function pointerDrawingEdit(page) {
  await expect(page.locator('#tegaki-cursor-layer')).toBeVisible();
  const before = await canvasProof(page);
  assert.equal(before.width, 400); assert.equal(before.height, 400); assert.ok(before.ink > 100);
  // Add a trusted pointer stroke below the existing source drawing.
  const target = await page.locator('#tegaki-canvas').boundingBox(); assert.ok(target);
  await page.mouse.move(target.x + 55, target.y + 335); await page.mouse.down();
  await page.mouse.move(target.x + 265, target.y + 325, { steps: 35 }); await page.mouse.up();
  await page.mouse.move(target.x + 85, target.y + 355); await page.mouse.down();
  await page.mouse.move(target.x + 250, target.y + 365, { steps: 35 }); await page.mouse.up();
  // Source oe_time rounds elapsed wall time to seconds. Keep a real trusted
  // stroke moving for >=1.1s so a legitimate fast edit cannot round to zero.
  // Bound both the movement count and the short interval between moves.
  const started = performance.now();
  await page.mouse.move(target.x + 80, target.y + 345); await page.mouse.down();
  let steps = 0;
  do {
    await page.mouse.move(target.x + (steps % 2 ? 255 : 85), target.y + 335 + (steps % 4) * 10, { steps: 3 });
    steps++;
    if (performance.now() - started < 1_150) await new Promise(resolve => setTimeout(resolve, 35));
  } while (performance.now() - started < 1_150 && steps < 48);
  await page.mouse.up();
  assert.ok(performance.now() - started >= 1_100, 'trusted pointer activity must span at least 1.1 seconds');
  const proof = await canvasProof(page);
  assert.equal(proof.width, 400); assert.equal(proof.height, 400); assert.equal(proof.replay, false);
  assert.ok(proof.ink > before.ink + 100, 'editing must add real pixels to the imported drawing');
  assert.notEqual(proof.hash, before.hash);
  return proof;
}

// Build the seed through an actual browser canvas, then hand its bounded PNG
// bytes to public intake. The edit must add real Tegaki pointer strokes later.
export async function seedDrawingPng(page) {
  const bytes = await page.evaluate(async () => {
    const canvas = document.createElement('canvas'); canvas.width = canvas.height = 400;
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = '#ffffff'; ctx.fillRect(0, 0, 400, 400);
    ctx.fillStyle = '#000000'; ctx.fillRect(55, 60, 155, 95);
    ctx.fillStyle = '#000000'; ctx.fillRect(95, 190, 175, 45);
    const blob = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
    if (!blob || blob.type !== 'image/png' || blob.size < 33 || blob.size > 8_388_608) throw new Error('Invalid seed PNG.');
    return [...new Uint8Array(await blob.arrayBuffer())];
  });
  const buffer = Buffer.from(bytes), proof = await decodedPngProof(page, buffer);
  assert.equal(proof.width, 400); assert.equal(proof.height, 400); assert.ok(proof.ink > 100);
  assert.ok(buffer.length >= 33 && buffer.length <= 8_388_608);
  return { buffer, proof };
}

export async function decodedPngProof(page, bytes) {
  return page.evaluate(async values => {
    const image = await createImageBitmap(new Blob([new Uint8Array(values)], { type: 'image/png' }));
    const canvas = document.createElement('canvas'); canvas.width = image.width; canvas.height = image.height;
    const context = canvas.getContext('2d'); context.drawImage(image, 0, 0); image.close();
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
    let ink = 0;
    for (let index = 0; index < pixels.length; index += 4) if (pixels[index + 3] && Math.min(...pixels.slice(index, index + 3)) < 240) ink++;
    const hash = [...new Uint8Array(await crypto.subtle.digest('SHA-256', pixels))].map(value => value.toString(16).padStart(2, '0')).join('');
    return { width: canvas.width, height: canvas.height, ink, hash };
  }, [...bytes]);
}

export async function closePainter(page, accept) {
  const pending = page.waitForEvent('dialog');
  const click = page.locator('#tegaki-menu-bar .tegaki-mb-btn').filter({ hasText: /^Close$/ }).click();
  const dialog = await pending; assert.equal(dialog.type(), 'confirm');
  assert.equal(dialog.message(), 'Are you sure? Your work will be lost.');
  await (accept ? dialog.accept() : dialog.dismiss()); await click;
}

// Capture before handing the unchanged real response to the client. The result
// lives in Node, so ordinary successful posting may navigate immediately.
let responseSequence = 0;
export async function observeDrawingResponse(page, url, stage, { annotation = false } = {}) {
  const name = `ownedDrawingResponse${++responseSequence}`;
  let finish;
  const pending = new Promise(resolve => { finish = resolve; });
  await page.exposeFunction(name, value => finish(value));
  await page.evaluate(({ name, url, annotation }) => {
    const original = window.fetch;
    window.fetch = async (...args) => {
      // Retain only the two harmless Oekaki fields. Never inspect, store or
      // report the one-use upload capability in the browser or Node observer.
      const metadata = annotation && String(args[0]) === url && args[1]?.body instanceof FormData
        ? { oe_src: args[1].body.get('oe_src'), oe_time: args[1].body.get('oe_time') } : null;
      const response = await original(...args);
      if (String(args[0]) === url && args[1]?.method === 'POST') {
        window.fetch = original;
        const captured = { status: response.status, type: response.headers.get('content-type') || '' };
        if (metadata) captured.annotation = metadata;
        try { captured.text = await response.clone().text(); } catch { captured.failed = true; }
        await window[name](captured);
      }
      return response;
    };
  }, { name, url, annotation });
  return async () => {
    const captured = await pending;
    const { ownedUploadResponse } = await import('../owned-upload-response.mjs');
    const result = await ownedUploadResponse({ status: () => captured.status, headers: () => ({ 'content-type': captured.type }),
      json: async () => { if (captured.failed) throw new Error('Body unavailable'); return JSON.parse(captured.text); } }, stage);
    if (stage === 'post' && !Number.isSafeInteger(result.result?.pid)) {
      // The source endpoint also reports rejections as HTTP 200 JSON. Emit
      // only fixed categories, never its arbitrary message or capability data.
      const error = typeof result.result?.error === 'string' ? result.result.error : '';
      const category = error === 'Invalid posting timestamp.' ? 'timestamp'
        : error.startsWith('Error: You must wait ') ? 'cooldown'
          : error === 'Storage is unavailable. Try again later.' ? 'storage'
            : error === 'Anonymous authorization changed.' ? 'session' : 'other';
      console.error(`OWNED_DRAWING_POST category=${category}`);
    }
    if (annotation) result.annotation = captured.annotation;
    return result;
  };
}

export async function exportDrawingProof(page, expected) {
  const exported = page.waitForEvent('download');
  await page.locator('#tegaki-menu-bar .tegaki-mb-btn').filter({ hasText: /^Export$/ }).click();
  const download = await exported;
  assert.match(download.suggestedFilename(), /\.png$/);
  const stream = await download.createReadStream(); assert.ok(stream);
  const chunks = []; let total = 0;
  for await (const chunk of stream) { total += chunk.length; assert.ok(total <= 8388608); chunks.push(chunk); }
  const proof = await decodedPngProof(page, Buffer.concat(chunks));
  assert.equal(proof.width, expected.width); assert.equal(proof.height, expected.height);
  assert.ok(proof.ink > 100); assert.equal(proof.hash, expected.hash);
  assert.equal((await canvasProof(page)).hash, expected.hash, 'Export leaves the retained canvas unchanged');
  await download.delete();
}

// Observe only the real cancellation transport; never read or log its request
// body (which contains the capability) or an arbitrary server error body.
export function observeDrawingCancellation(page, url) {
  let state = 'none';
  page.on('request', request => { if (request.url() === url && request.method() === 'POST') state = 'pending'; });
  page.on('response', response => {
    if (response.url() === url && response.request().method() === 'POST') {
      const status = response.status();
      state = Number.isInteger(status) && status >= 100 && status <= 599 ? String(status) : 'other';
    }
  });
  page.on('requestfailed', request => { if (request.url() === url && request.method() === 'POST' && state === 'pending') state = 'failed'; });
  return () => state;
}

export function drawingStatusCategory(text) {
  const categories = { '': 'empty', 'tegaki.png queued for processing.': 'queued',
    'Canceling drawing upload…': 'canceling', 'Checking tegaki.png…': 'checking',
    'Uploading tegaki.png…': 'uploading', 'Upload could not be canceled. Try again.': 'cancel-error',
    'The drawing editor could not be loaded. Try Draw again.': 'editor-error',
  };
  return Object.hasOwn(categories, text) ? categories[text] : 'other';
}

export async function reportDrawingEditFailure(page, controls, cancellation, report = console.error) {
  // Diagnostics must not replace the original assertion, even on a closed page.
  try {
    const ui = drawingStatusCategory(await controls.status.textContent());
    const visibility = async selector => {
      const element = page.locator(selector);
      return await element.count() === 0 ? 'absent' : await element.isVisible() ? 'visible' : 'hidden';
    };
    const editor = await visibility('#tegaki'), cursor = await visibility('#tegaki-cursor-layer');
    const raw = await page.locator('html').getAttribute('data-native-drawing-active');
    const active = ['true', 'false'].includes(raw) ? raw : 'other';
    const value = cancellation(), cancel = /^(?:none|pending|failed|other|[1-5][0-9]{2})$/.test(value) ? value : 'other';
    report(`OWNED_DRAWING_EDIT cancel=${cancel} ui=${ui} editor=${editor} cursor=${cursor} active=${active}`);
  } catch { report('OWNED_DRAWING_EDIT unavailable'); }
}
