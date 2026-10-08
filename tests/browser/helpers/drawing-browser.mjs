import assert from 'node:assert/strict';
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
export async function observeDrawingResponse(page, url, stage) {
  const name = `ownedDrawingResponse${++responseSequence}`;
  let finish;
  const pending = new Promise(resolve => { finish = resolve; });
  await page.exposeFunction(name, value => finish(value));
  await page.evaluate(({ name, url }) => {
    const original = window.fetch;
    window.fetch = async (...args) => {
      const response = await original(...args);
      if (String(args[0]) === url && args[1]?.method === 'POST') {
        window.fetch = original;
        const captured = { status: response.status, type: response.headers.get('content-type') || '' };
        try { captured.text = await response.clone().text(); } catch { captured.failed = true; }
        await window[name](captured);
      }
      return response;
    };
  }, { name, url });
  return async () => {
    const captured = await pending;
    const { ownedUploadResponse } = await import('../owned-upload-response.mjs');
    return ownedUploadResponse({ status: () => captured.status, headers: () => ({ 'content-type': captured.type }),
      json: async () => { if (captured.failed) throw new Error('Body unavailable'); return JSON.parse(captured.text); } }, stage);
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
