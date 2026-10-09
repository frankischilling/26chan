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

// These are local scanner contracts, not an oracle for the absent MathJax 2.6
// tex2jax implementation. An opener inside a span is ordinary TeX input.
test('local nested delimiters use the first matching closer without recursive expansion', () => {
  const cases = [
    ['[math]a[math]b[/math]c[/math]', ['a[math]b', false], 'c[/math]'],
    ['[eqn]a[eqn]b[/eqn]c[/eqn]', ['a[eqn]b', true], 'c[/eqn]'],
    ['[math]a[eqn]b[/eqn]c[/math]', ['a[eqn]b[/eqn]c', false], ''],
    ['[eqn]a[math]b[/math]c[/eqn]', ['a[math]b[/math]c', true], ''],
    ['[math]a[eqn]b[/math]c[/eqn]', ['a[eqn]b', false], 'c[/eqn]'],
    ['[eqn]a[math]b[/eqn]c[/math]', ['a[math]b', true], 'c[/math]'],
  ];
  for (const [input, expected, trailing] of cases) {
    const spans = mathSpans(input);
    assert.equal(spans.length, 1, input);
    assert.deepEqual([spans[0].tex, spans[0].display], expected, input);
    assert.equal(spans[0].start, 0, input);
    assert.equal(input.slice(spans[0].end), trailing, input);
    const tag = expected[1] ? 'eqn' : 'math';
    assert.equal(input.slice(spans[0].start, spans[0].end), `[${tag}]${expected[0]}[/${tag}]`);
  }
});

test('local malformed outer delimiters preserve offsets and allow complete inner or later spans', () => {
  const input = '[math]a[eqn]b[/eqn] tail [math]z[/math]';
  assert.deepEqual(mathSpans(input), [{ start: 0, end: input.length,
    tex: 'a[eqn]b[/eqn] tail [math]z', display: false }]);
  const incomplete = '[math]a[eqn]b[/eqn]';
  assert.deepEqual(mathSpans(incomplete), [{ start: 7, end: incomplete.length, tex: 'b', display: true }]);
  for (const input of ['[/math][math]a', '[math][eqn]a', '[math]a[/eqn]', '[MATH]a[/MATH]']) {
    assert.deepEqual(mathSpans(input), [], input);
  }
  assert.deepEqual(mathSpans('[/eqn] [math]z[/math]'), [{ start: 7, end: 21, tex: 'z', display: false }]);
});

test('local deep delimiter scans cap input and span count independently of worker TeX limits', () => {
  const depth = 4096;
  const deep = '[math]'.repeat(depth) + 'x' + '[/math]'.repeat(depth);
  const spans = mathSpans(deep);
  assert.equal(spans.length, 1);
  assert.deepEqual(spans[0], { start: 0, end: depth * 6 + 8,
    tex: '[math]'.repeat(depth - 1) + 'x', display: false });
  assert.ok(spans[0].tex.length > 4096); // Scanning is not worker admission.
  const atLimit = deep.padEnd(65536, ' ');
  assert.deepEqual(mathSpans(atLimit), spans);
  assert.deepEqual(mathSpans(atLimit + ' '), []);
  const unclosed = '[math]'.repeat(Math.floor(65536 / 6));
  assert.deepEqual(mathSpans(unclosed), []);
  const unit = '[math][math]x[/math][/math]';
  assert.equal(mathSpans(unit.repeat(MATH_LIMITS.expressions)).length, MATH_LIMITS.expressions);
  assert.deepEqual(mathSpans(unit.repeat(MATH_LIMITS.expressions + 1)), []);
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
