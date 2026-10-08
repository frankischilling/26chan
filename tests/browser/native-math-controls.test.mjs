import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { mathSpans, MATH_LIMITS } from '../../apps/public/client/native-math.js';

test('literal delimiter scanning is bounded, case sensitive, and keeps exact offsets', () => {
  assert.deepEqual(mathSpans('[Math]a[/Math] [math]x[/math] [eqn]y\nz[/eqn]').map(({ tex, display }) => [tex, display]), [['x', false], ['y\nz', true]]);
  const input = 'before [math]x[/math] after', [span] = mathSpans(input);
  assert.equal(input.slice(span.start, span.end), '[math]x[/math]');
  assert.deepEqual(mathSpans('[math]unterminated'), []);
  assert.deepEqual(mathSpans('[eqn]mismatch[/math]'), []);
  assert.equal(mathSpans('[math]x[/math]'.repeat(MATH_LIMITS.expressions)).length, MATH_LIMITS.expressions);
  assert.deepEqual(mathSpans('[math]x[/math]'.repeat(MATH_LIMITS.expressions + 1)), []);
  assert.deepEqual(mathSpans('x'.repeat(65537)), []);
});

async function source() {
  const base = new URL('../../apps/public/client/', import.meta.url);
  const data = source => `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`;
  const markup = data(await readFile(new URL('native-wordfilter-markup.js', base), 'utf8'));
  const projection = data((await readFile(new URL('native-comment-projection.js', base), 'utf8')).replace('./native-wordfilter-markup.js', markup));
  const schema = data(await readFile(new URL('math/schema.mjs', base), 'utf8'));
  return { projection, math: data((await readFile(new URL('native-math.js', base), 'utf8'))
    .replace('./native-comment-projection.js', projection).replace('./math/schema.mjs', schema)) };
}

test('preview has separate debounced input, cancels stale output, and disabled pages start no worker', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); await page.goto('about:blank');
    const result = await page.evaluate(async ({ projection, math }) => {
      const { createCommentProjection } = await import(projection), { mountNativeMath, mathSpans } = await import(math);
      const workers = []; window.IntersectionObserver = undefined;
      window.Worker = class { constructor(url) { this.url = url; this.sent = []; workers.push(this); } postMessage(job) { this.sent.push(job); } terminate() { this.terminated = true; } };
      const root = document.createElement('div'); root.className = 'board'; document.body.append(root);
      const p = createCommentProjection();
      const disabled = mountNativeMath({ root, projection: p }) === null && workers.length === 0;
      document.body.dataset.mathTags = '1'; const controller = mountNativeMath({ root, projection: p });
      const lazy = workers.length === 0;
      controller.openPreview(); const input = document.getElementById('input-tex-preview');
      input.value = '[math]a[/math]'; input.dispatchEvent(new Event('input'));
      input.value = '[eqn]b[/eqn]'; input.dispatchEvent(new Event('input'));
      const debounced = workers.length === 0;
      await new Promise(resolve => setTimeout(resolve, 70));
      const latest = workers[0].sent[0], worker = workers[0];
      input.value = '[math]new[/math]'; input.dispatchEvent(new Event('input'));
      const cancelled = worker.terminated === true;
      controller.closePreview(); await new Promise(resolve => setTimeout(resolve, 70));
      const closed = !document.getElementById('tex-preview-cnt') && workers.length === 1;
      controller.disconnect();
      return { disabled, lazy, debounced, latest, cancelled, closed,
        spans: mathSpans('[Math]x[/Math] [math]y[/math] [eqn]z[/eqn]').map(({ tex, display }) => [tex, display]) };
    }, await source());
    assert.equal(result.disabled, true); assert.equal(result.lazy, true); assert.equal(result.debounced, true);
    assert.equal(result.latest.tex, 'b'); assert.equal(result.latest.display, true);
    assert.equal(result.cancelled, true); assert.equal(result.closed, true);
    assert.deepEqual(result.spans, [['y', false], ['z', true]]);
  } finally { await browser.close(); }
});
