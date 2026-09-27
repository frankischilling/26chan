import test, { before, after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

let browser;
before(async () => { browser = await chromium.launch({ headless: true }); });
after(async () => { await browser?.close(); });
const root = new URL('../../', import.meta.url);
const source = await readFile(new URL('apps/public/client/native-images.js', root), 'utf8');
const projection = await readFile(new URL('apps/public/client/native-comment-projection.js', root), 'utf8');
const quotes = await readFile(new URL('apps/public/static/native-filter.v1.js', root), 'utf8');
const css = await readFile(new URL('apps/public/static/board.css', root), 'utf8');
// A transparent synthetic 1x1 PNG. All requests are intercepted in this context.
const pixel = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==', 'base64');

async function fixture(t, { limits = {}, config = {}, count = 3 } = {}) {
  const context = await browser.newContext();
  t.after(() => context.close());
  const page = await context.newPage();
  const requests = [], modes = new Map(), held = [];
  await page.route('**/*', async route => {
    const url = route.request().url(); requests.push(url);
    if (!url.startsWith('https://media.test/')) { await route.abort(); return; }
    const mode = modes.get(new URL(url).pathname);
    if (mode === 'hold') { held.push(route); return; }
    if (mode === 'fail') { await route.fulfill({ status: 404, body: 'Owned missing image' }); return; }
    await route.fulfill({ contentType: 'image/png', body: pixel });
  });
  await page.goto('about:blank');
  await page.addStyleTag({ content: css });
  await page.evaluate(async ({ source, projection, quotes, limits, config, count }) => {
    const module = async text => {
      const url = URL.createObjectURL(new Blob([text], { type: 'text/javascript' }));
      try { return await import(url); } finally { URL.revokeObjectURL(url); }
    };
    const images = await module(source), ownership = await module(projection);
    window.quotes = await module(quotes);
    // Fixed synthetic posts only; application code never uses this HTML path.
    document.body.innerHTML = '<main class="board">' + Array.from({ length: count }, (_, index) => {
      const id = index + 1, type = id === 1 ? 'op' : 'reply';
      return `<article class="postContainer ${type}Container" id="pc${id}"><div class="post ${type}" id="p${id}"><div class="postInfo" id="pi${id}"><span class="name">Owned</span></div><div class="file" id="f${id}"><p>File: <a href="https://media.test/demo/${id}.png" rel="noopener noreferrer">Owned.png</a></p><a class="fileThumb" href="https://media.test/demo/${id}.png" rel="noopener noreferrer"><img src="https://media.test/demo/${id}s.jpg" alt="Owned" width="10" height="10" loading="lazy"></a></div><blockquote class="postMessage" id="m${id}">Owned</blockquote></div></article>`;
    }).join('') + '</main>';
    window.imageConfig = config; window.imageOwnership = ownership.createCommentProjection();
    window.imageController = images.mountNativeImages({ root: document.querySelector('.board'),
      mediaOrigin: 'https://media.test', settings: () => window.imageConfig, projection: window.imageOwnership, limits });
    window.clickImage = id => {
      const anchor = document.querySelector(`#f${id} a.fileThumb`);
      let intercepted;
      const preventNavigation = event => { intercepted = event.defaultPrevented; event.preventDefault(); };
      document.addEventListener('click', preventNavigation, { once: true });
      anchor.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 }));
      return intercepted;
    };
  }, { source, projection, quotes, limits, config, count });
  await page.waitForFunction(() => [...document.querySelectorAll('.fileThumb img')].every(img => img.complete && img.naturalWidth > 0));
  return { page, requests, modes, held };
}

test('expanded images retain the original thumbnail and cannot enter quote recipes', async t => {
  const { page, requests } = await fixture(t);
  const before = await page.locator('#f1 .fileThumb img').getAttribute('src');
  assert.equal(await page.evaluate(() => clickImage(1)), true);
  await page.waitForSelector('#f1 .expanded-thumb:not([hidden])');
  const result = await page.evaluate(() => {
    const image = document.querySelector('#f1 .expanded-thumb');
    const tree = quotes.localQuoteTree(document.getElementById('pc1'),
      { origin: 'https://boards.test', mediaOrigin: 'https://media.test', board: 'demo', thread: '1' }, '1', imageOwnership);
    return { owned: imageOwnership.has(image), tree: JSON.stringify(tree), original: document.querySelector('#f1 img').getAttribute('src') };
  });
  assert.equal(result.owned, true); assert.equal(result.original, before);
  assert.equal((result.tree.match(/"tag":"img"/g) || []).length, 1);
  assert.ok(result.tree.includes('1s.jpg')); assert.ok(!result.tree.includes('expanded-thumb'));
  assert.equal(requests.filter(url => url.endsWith('/1.png')).length, 1);
  await page.evaluate(() => clickImage(1));
  assert.equal(await page.locator('.expanded-thumb').count(), 0);
  assert.equal(await page.locator('#f1 .fileThumb img').getAttribute('src'), before);
});

test('pending image slots have a deadline, release capacity, and permit a healthy explicit retry', async t => {
  const { page, requests, modes } = await fixture(t, { limits: { expansions: 2, loadMs: 250 } });
  modes.set('/demo/1.png', 'hold'); modes.set('/demo/2.png', 'hold');
  const admission = await page.evaluate(() => [clickImage(1), clickImage(2), clickImage(3)]);
  assert.deepEqual(admission, [true, true, true]);
  assert.equal(await page.locator('.expanded-thumb').count(), 2);
  assert.equal(await page.locator('.nativeImageFeedback').textContent(), 'Close another expanded image before opening this one.');
  assert.ok(!requests.some(url => url.endsWith('/3.png')));
  await page.waitForFunction(() => document.querySelectorAll('.expanded-thumb,.nativeImageLoading').length === 0);
  assert.match(await page.locator('.nativeImageFeedback').textContent(), /could not be loaded/);
  modes.delete('/demo/1.png'); await page.evaluate(() => clickImage(1));
  await page.waitForSelector('#f1 .expanded-thumb:not([hidden])');
  assert.equal(await page.locator('.nativeImageLoading').count(), 0);
});

test('deletion, suspension, disabled settings and changed URLs cancel pending images before late completion', async t => {
  const { page, modes, held } = await fixture(t);
  for (const id of [1, 2, 3]) modes.set(`/demo/${id}.png`, 'hold');
  await page.evaluate(() => { clickImage(1); clickImage(2); });
  await page.waitForFunction(() => document.querySelectorAll('.nativeImageLoading').length === 2);
  await page.evaluate(() => { document.getElementById('pc1').remove(); document.getElementById('pc2').hidden = true; });
  await page.waitForFunction(() => document.querySelectorAll('.expanded-thumb').length === 0);
  await page.evaluate(() => clickImage(3));
  await page.waitForSelector('#f3 .nativeImageLoading');
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  assert.equal(await page.locator('.expanded-thumb').count(), 0);
  assert.equal(await page.evaluate(() => clickImage(3)), false);
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  await page.evaluate(() => clickImage(3));
  await page.waitForSelector('#f3 .nativeImageLoading');
  await page.evaluate(() => { imageConfig = { disableAll: true }; window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' })); });
  assert.equal(await page.locator('.expanded-thumb').count(), 0);
  await page.evaluate(() => { imageConfig = {}; clickImage(3); document.querySelector('#f3 .fileThumb').href = 'https://foreign.test/demo/3.png'; });
  await page.waitForFunction(() => document.querySelectorAll('.expanded-thumb').length === 0);
  for (const route of held) await route.fulfill({ contentType: 'image/png', body: pixel }).catch(() => {});
  assert.equal(await page.locator('.expanded-thumb').count(), 0);
  assert.equal(await page.evaluate(() => clickImage(3)), false);
});

test('hover errors and leaving the thumbnail remove their request and never grant arbitrary URL authority', async t => {
  const { page, requests, modes } = await fixture(t, { config: { imageHover: true } });
  modes.set('/demo/1.png', 'hold');
  await page.locator('#f1 .fileThumb').dispatchEvent('mouseover');
  await page.waitForSelector('#image-hover', { state: 'attached' });
  await page.locator('#f1 .fileThumb').dispatchEvent('mouseout');
  assert.equal(await page.locator('#image-hover').count(), 0);
  modes.set('/demo/2.png', 'fail');
  await page.locator('#f2 .fileThumb').dispatchEvent('mouseover');
  await page.waitForFunction(() => document.querySelector('.nativeImageFeedback')?.textContent.includes('preview could not'));
  assert.equal(await page.locator('#image-hover').count(), 0);
  const denied = await page.evaluate(() => {
    const anchor = document.querySelector('#f3 .fileThumb');
    return ['https://foreign.test/demo/3.png', 'https://media.test/demo/3.svg', 'https://media.test/demo/3.png?secret=x',
      'https://media.test/demo/../demo/3.png', 'https://media.test/demo/9223372036854775808.png'].map(url => {
      anchor.setAttribute('href', url); anchor.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
      return clickImage(3);
    });
  });
  assert.deepEqual(denied, Array(5).fill(false));
  assert.ok(requests.every(url => /^https:\/\/media\.test\/demo\/[1-3](s\.jpg|\.png)$/.test(url)));
  assert.equal(await page.locator('#image-hover,.expanded-thumb').count(), 0);
});

test('spoiler filename reveal preserves the original caption and inert quote recipe through concealment', async t => {
  const { page } = await fixture(t);
  const filename = 'Owned λ & <label>.png';
  await page.evaluate(filename => {
    const file = document.getElementById('f1'), caption = file.querySelector('p a');
    file.querySelector('a.fileThumb').remove();
    caption.textContent = 'Spoiler Image';
    Object.assign(file.dataset, { imageSpoiler: 'true', imageFilename: filename,
      thumbnailWidth: '10', thumbnailHeight: '10' });
    const details = document.createElement('details'), summary = document.createElement('summary'), link = document.createElement('a');
    summary.textContent = 'Spoiler image'; link.textContent = 'View spoiler image';
    link.href = caption.href; link.rel = 'noopener noreferrer';
    details.append(summary, link); file.append(details);
    imageConfig = { revealSpoilers: true }; imageController.refresh();
  }, filename);
  await page.waitForSelector('#f1.nativeSpoilerRevealed');
  assert.equal(await page.locator('#f1 > p > a').nth(1).innerText(), filename);
  assert.equal(await page.locator('#f1 > p > a').first().isVisible(), false);
  assert.equal(await page.locator('#f1 .fileThumb img').getAttribute('alt'), filename);
  const recipe = await page.evaluate(() => {
    const tree = quotes.localQuoteTree(document.getElementById('pc1'),
      { origin: 'https://boards.test', mediaOrigin: 'https://media.test', board: 'demo', thread: '1' }, '1', imageOwnership);
    return JSON.stringify(tree);
  });
  assert.ok(recipe.includes('Spoiler Image'));
  assert.ok(!recipe.includes(filename));
  assert.ok(!recipe.includes('1s.jpg'));
  await page.evaluate(() => { imageConfig = {}; imageController.refresh(); });
  assert.equal(await page.locator('#f1 > p > a').count(), 1);
  assert.equal(await page.locator('#f1 > p > a').innerText(), 'Spoiler Image');
  assert.equal(await page.locator('#f1 > p > a').isVisible(), true);
  assert.equal(await page.locator('#f1 > a.fileThumb').count(), 0);
  await page.evaluate(() => {
    document.getElementById('f1').dataset.imageFilename = 'é'.repeat(128);
    imageConfig = { revealSpoilers: true }; imageController.refresh();
  });
  assert.equal(await page.locator('#f1 > p > a').count(), 1);
  assert.equal(await page.locator('#f1 > a.fileThumb').count(), 0);
});
