import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { createCommentProjection } from '../../apps/public/client/native-comment-projection.js';

test('source text is charged and escaped even when the live Text is empty', () => {
  const projection = createCommentProjection();
  const node = { nodeType: 3, data: '', childNodes: [] };
  const message = { childNodes: [node] };
  const release = projection.trackText(node, () => '[math]x < y & z[/math]');
  assert.equal(projection.html(message), '[math]x &lt; y &amp; z[/math]');
  assert.equal(projection.text(node), '[math]x < y & z[/math]');
  assert.equal(projection.isTrackedText(node), true);
  release(); assert.equal(projection.isTrackedText(node), false);
  projection.trackText(node, () => 'x'.repeat(65537));
  assert.throws(() => projection.html(message), RangeError);
});

async function sources() {
  const base = new URL('../../apps/public/client/', import.meta.url);
  const data = source => `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`;
  const markup = data(await readFile(new URL('native-wordfilter-markup.js', base), 'utf8'));
  const projection = data((await readFile(new URL('native-comment-projection.js', base), 'utf8')).replace('./native-wordfilter-markup.js', markup));
  const schema = data(await readFile(new URL('math/schema.mjs', base), 'utf8'));
  return { projection, math: data((await readFile(new URL('native-math.js', base), 'utf8'))
    .replace('./native-comment-projection.js', projection).replace('./math/schema.mjs', schema)) };
}

test('initial pageshow cannot replenish exhausted worker retries', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); await page.goto('about:blank');
    const result = await page.evaluate(async ({ projection, math }) => {
      const { createCommentProjection } = await import(projection);
      const { mountNativeMath } = await import(math);
      document.body.innerHTML = '<div class="board"><blockquote class="postMessage"></blockquote></div>';
      document.querySelector('.postMessage').textContent = '[math]x[/math] '.repeat(4);
      document.body.dataset.mathTags = '1'; window.IntersectionObserver = undefined;
      const workers = [];
      window.Worker = class {
        constructor() { workers.push(this); }
        postMessage() { queueMicrotask(() => this.onerror?.()); }
        terminate() { this.terminated = true; }
      };
      const root = document.querySelector('.board'), message = root.querySelector('.postMessage');
      const controller = mountNativeMath({ root, projection: createCommentProjection(), board: 'sci' });
      await new Promise(resolve => setTimeout(resolve, 30));
      const initial = workers.length;
      window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: false }));
      message.textContent = '[math]y[/math]'; controller.refresh();
      await new Promise(resolve => setTimeout(resolve, 30));
      const after = workers.length;
      controller.disconnect();
      return { initial, after, literal: message.textContent, allStopped: workers.every(worker => worker.terminated) };
    }, await sources());
    assert.deepEqual(result, { initial: 4, after: 4, literal: '[math]y[/math]', allStopped: true });
  } finally { await browser.close(); }
});

test('math projection preserves literal Text identity and ignores only owned UI', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); await page.goto('about:blank');
    const result = await page.evaluate(async ({ projection }) => {
      const { createCommentProjection } = await import(projection), p = createCommentProjection();
      const message = document.createElement('blockquote'), source = document.createTextNode('[math]x[/math]');
      message.append(source); const original = p.html(message);
      const release = p.trackText(source, () => '[math]x[/math]'); source.data = '';
      const ui = document.createElement('span'); ui.innerHTML = '<svg><path d="M0 0"></path></svg>';
      p.claim(ui, {}); message.append(ui);
      const good = { html: p.html(message), text: p.text(message), clone: p.clone(message).innerHTML,
        same: message.firstChild === source, original };
      const forged = ui.cloneNode(true); message.append(forged);
      let htmlRejected = false, textRejected = false, cloneRejected = false;
      try { p.html(message); } catch { htmlRejected = true; }
      try { p.text(message); } catch { textRejected = true; }
      try { p.clone(message); } catch { cloneRejected = true; }
      release(); return { good, htmlRejected, textRejected, cloneRejected };
    }, await sources());
    assert.deepEqual(result.good, { html: '[math]x[/math]', text: '[math]x[/math]', clone: '[math]x[/math]', same: true, original: '[math]x[/math]' });
    assert.equal(result.htmlRejected, true); assert.equal(result.textRejected, true); assert.equal(result.cloneRejected, true);
  } finally { await browser.close(); }
});

test('controller fences full source mutations and restores output on removal and page lifecycle', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); await page.goto('about:blank');
    const result = await page.evaluate(async ({ projection, math }) => {
      const { createCommentProjection } = await import(projection), { mountNativeMath } = await import(math);
      const workers = []; window.IntersectionObserver = undefined;
      window.Worker = class {
        constructor(url) { this.url = url; this.sent = []; workers.push(this); }
        postMessage(job) { this.sent.push(job); }
        terminate() { this.terminated = true; }
      };
      document.body.dataset.mathTags = '1';
      const root = document.createElement('div'); root.className = 'board'; document.body.append(root);
      const message = document.createElement('blockquote'); message.className = 'postMessage';
      message.innerHTML = '<span>[math]x[/math]</span>'; root.append(message);
      const p = createCommentProjection(), controller = mountNativeMath({ root, projection: p, board: 'sci' });
      const geometry = { viewBox: [0, 0, 10, 10], width: 1, height: 1, children: [{ tag: 'path', attrs: { d: 'M0 0L10 10' } }] };
      const first = workers[0], stale = first.sent[0];
      message.firstChild.setAttribute('class', 'changed');
      first.onmessage({ data: { id: stale.id, ok: true, geometry } });
      const staleRejected = !message.querySelector('svg');
      controller.refresh(); const next = first.sent.at(-1);
      if (!message.querySelector('svg')) first.onmessage({ data: { id: next.id, ok: true, geometry } });
      const html = p.html(message), rendered = !!message.querySelector('svg');
      window.dispatchEvent(new Event('pagehide'));
      const restored = message.innerHTML, terminated = first.terminated;
      window.dispatchEvent(new Event('pageshow'));
      const resumed = !!message.querySelector('svg'); // Validated cache is reusable.
      message.remove(); controller.refresh();
      const removedRestored = !message.querySelector('svg') && message.textContent === '[math]x[/math]';
      controller.disconnect(); return { staleRejected, html, rendered, restored, terminated, resumed, removedRestored };
    }, await sources());
    assert.equal(result.staleRejected, true); assert.equal(result.rendered, true);
    assert.equal(result.html, '<span class="changed">[math]x[/math]</span>');
    assert.equal(result.restored, result.html); assert.equal(result.terminated, true);
    assert.equal(result.resumed, true); assert.equal(result.removedRestored, true);
  } finally { await browser.close(); }
});

test('malformed geometry never reaches the DOM, including resource attributes', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); await page.goto('about:blank');
    const result = await page.evaluate(async ({ projection, math }) => {
      const { createCommentProjection } = await import(projection), { mountNativeMath } = await import(math);
      const workers = []; window.IntersectionObserver = undefined;
      window.Worker = class { constructor() { workers.push(this); } postMessage(job) { this.job = job; } terminate() { this.terminated = true; } };
      const root = document.createElement('div'); root.className = 'board'; document.body.append(root);
      const message = document.createElement('blockquote'); message.className = 'postMessage'; message.textContent = '[math]x[/math]'; root.append(message);
      document.body.dataset.mathTags = '1'; const p = createCommentProjection();
      const controller = mountNativeMath({ root, projection: p });
      workers[0].onmessage({ data: { id: workers[0].job.id, ok: true,
        geometry: { viewBox: [0, 0, 10, 10], width: 1, height: 1, children: [{ tag: 'path', attrs: { d: 'M0 0', href: 'https://invalid.example' } }] } } });
      const result = { html: message.innerHTML, source: p.html(message), stopped: workers[0].terminated, svg: message.querySelectorAll('svg').length };
      controller.disconnect(); return result;
    }, await sources());
    assert.deepEqual(result, { html: '[math]x[/math]', source: '[math]x[/math]', stopped: true, svg: 0 });
  } finally { await browser.close(); }
});

test('viewport target retention is finite and removal frees admission', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage(); await page.goto('about:blank');
    const result = await page.evaluate(async ({ projection, math }) => {
      const { createCommentProjection } = await import(projection), { mountNativeMath, MATH_LIMITS } = await import(math);
      const observed = new Set(); let workerCount = 0;
      window.IntersectionObserver = class { observe(node) { observed.add(node); } unobserve(node) { observed.delete(node); } disconnect() { observed.clear(); } };
      window.Worker = class { constructor() { workerCount++; } };
      const root = document.createElement('div'); root.className = 'board'; document.body.append(root);
      for (let index = 0; index < MATH_LIMITS.messages + 10; index++) {
        const message = document.createElement('blockquote'); message.className = 'postMessage'; message.textContent = `[math]${index}[/math]`; root.append(message);
      }
      document.body.dataset.mathTags = '1'; const controller = mountNativeMath({ root, projection: createCommentProjection() });
      const initial = observed.size;
      for (const message of observed) message.remove();
      controller.refresh(); const afterRemoval = observed.size;
      controller.disconnect(); return { initial, afterRemoval, disposed: observed.size, workerCount, limit: MATH_LIMITS.messages };
    }, await sources());
    assert.equal(result.initial, result.limit); assert.equal(result.afterRemoval, 10);
    assert.equal(result.disposed, 0); assert.equal(result.workerCount, 0);
  } finally { await browser.close(); }
});
