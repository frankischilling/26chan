import test, { before, after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

let browser;
before(async () => { browser = await chromium.launch({ headless: true }); });
after(async () => { await browser?.close(); });
const root = new URL('../../', import.meta.url);
const spoilerAssets = await readFile(new URL('apps/public/client/native-spoiler-assets.js', root), 'utf8');
const assetUrl = `data:text/javascript;base64,${Buffer.from(spoilerAssets).toString('base64')}`;
const spoilerCache = (await readFile(new URL('apps/public/client/native-spoilers.js', root), 'utf8')).replace('./native-spoiler-assets.js', assetUrl);
const source = (await readFile(new URL('apps/public/client/native-images.js', root), 'utf8'))
  .replace('./native-spoiler-assets.js', assetUrl)
  .replace('./native-spoilers.js', `data:text/javascript;base64,${Buffer.from(spoilerCache).toString('base64')}`);
const wordfilters = await readFile(new URL('apps/public/client/native-wordfilter-markup.js', root), 'utf8');
const projection = (await readFile(new URL('apps/public/client/native-comment-projection.js', root), 'utf8'))
  .replace('./native-wordfilter-markup.js', `data:text/javascript;base64,${Buffer.from(wordfilters).toString('base64')}`);
const quotes = await readFile(new URL('apps/public/static/native-filter.v1.js', root), 'utf8');
const css = await readFile(new URL('apps/public/static/board.css', root), 'utf8');
// A transparent synthetic 1x1 PNG. All requests are intercepted in this context.
const pixel = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==', 'base64');
// Independent 1x1 red/blue GIF89a, 200 ms frames, infinite loop.
const ownedVideos = JSON.parse(await readFile(new URL('fixtures/owned-video-clips.json', root), 'utf8')).clips;
const animatedPixel = Buffer.from('47494638396101000100800000ff00000000ff21ff0b4e45545343415045322e30030100000021f90404140000002c000000000100010000020244010021f90404140000002c00000000010001000002024c01003b', 'hex');

async function fixture(t, { limits = {}, config = {}, count = 3, preview = false, gif = false, pdf = false, video = null, storageOrigin = false, fallback = false } = {}) {
  const context = await browser.newContext();
  t.after(() => context.close());
  const page = await context.newPage();
  const requests = [], modes = new Map(), held = [];
  await context.route('**/*', async route => {
    const url = route.request().url(); requests.push(url);
    if (storageOrigin && url === 'https://boards.test/') { await route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Owned video fixture</title>' }); return; }
    if (fallback && url.startsWith('https://boards.test/static/')) {
      const path = new URL(url).pathname;
      if (path === '/static/native-video-fallback.v1.js' || /^\/static\/ogv\/[a-z0-9.-]+\.(?:js|wasm)$/.test(path)) {
        await route.fulfill({ contentType: path.endsWith('.wasm') ? 'application/wasm' : 'text/javascript', body: await readFile(new URL(`apps/public${path}`, root)) }); return;
      }
    }
    if (!url.startsWith('https://media.test/')) { await route.abort(); return; }
    const mode = modes.get(new URL(url).pathname);
    if (mode === 'hold') { held.push(route); return; }
    if (mode === 'fail') { await route.fulfill({ status: 404, body: 'Owned missing image' }); return; }
    if (video && /\.(webm|mp4)$/.test(url)) {
      const ext = video.endsWith('.mp4') ? 'mp4' : 'webm';
      await route.fulfill({ contentType: `video/${ext}`, headers: { "access-control-allow-origin": "https://boards.test", "access-control-expose-headers": "Content-Length, Content-Range, Accept-Ranges" }, body: Buffer.from(ownedVideos[video].base64, 'base64') }); return;
    }
    await route.fulfill(url.endsWith('.gif') ? { contentType: 'image/gif', body: animatedPixel }
      : { contentType: 'image/png', body: pixel });
  });
  if (fallback) await page.addInitScript(() => { HTMLMediaElement.prototype.canPlayType = () => ''; });
  await page.goto(storageOrigin ? 'https://boards.test/' : 'about:blank');
  await page.addStyleTag({ content: css });
  await page.evaluate(async ({ source, projection, quotes, limits, config, count, preview, gif, pdf, video }) => {
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
    if (video) for (const anchor of document.querySelectorAll('.file a[href]')) {
      anchor.href = anchor.href.replace(/\.png$/, video.endsWith('.mp4') ? '.mp4' : '.webm');
    }
    if (pdf) for (const anchor of document.querySelectorAll('.file a[href]')) {
      anchor.href = anchor.href.replace(/\.png$/, '.pdf');
    }
    if (gif) for (const anchor of document.querySelectorAll('.file a[href]')) {
      anchor.href = anchor.href.replace(/\.png$/, '.gif');
    }
    window.imageConfig = config; window.imageOwnership = ownership.createCommentProjection();
    window.imageController = images.mountNativeImages({ root: document.querySelector('.board'),
      mediaOrigin: 'https://media.test', settings: () => window.imageConfig, projection: window.imageOwnership, limits, mobile: window.matchMedia('(max-width: 480px)'),
      ...(preview ? { previewRoot: () => document.getElementById('quote-preview') } : {}) });
    window.clickImage = id => {
      const anchor = document.querySelector(`#f${id} a.fileThumb`);
      let intercepted;
      const preventNavigation = event => { intercepted = event.defaultPrevented; event.preventDefault(); };
      document.addEventListener('click', preventNavigation, { once: true });
      anchor.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 }));
      return intercepted;
    };
  }, { source, projection, quotes, limits, config, count, preview, gif, pdf, video });
  await page.waitForFunction(() => [...document.querySelectorAll('.fileThumb img')].every(img => img.complete && img.naturalWidth > 0));
  return { page, requests, modes, held };
}

test('GIF expansion displays changing frames, keeps its PNG thumbnail and retires the GIF URL', async t => {
  const { page, requests } = await fixture(t, { gif: true, count: 1 });
  const thumbnail = await page.locator('#f1 .fileThumb img').getAttribute('src');
  assert.equal(await page.evaluate(() => clickImage(1)), true);
  const expanded = page.locator('#f1 .expanded-thumb:not([hidden])');
  await expanded.waitFor();
  assert.equal(await expanded.getAttribute('src'), 'https://media.test/demo/1.gif');
  const first = await expanded.screenshot();
  let changed = false;
  for (let attempt = 0; attempt < 20 && !changed; attempt++) {
    await page.waitForTimeout(60);
    changed = !(await expanded.screenshot()).equals(first);
  }
  assert.ok(changed, 'the visible GIF must display more than its first frame');
  assert.equal(await page.locator('#f1 .fileThumb img:not(.expanded-thumb)').getAttribute('src'), thumbnail);
  assert.equal(requests.filter(url => url.endsWith('/1.gif')).length, 1);
  assert.equal(await page.evaluate(() => imageController.retire('https://media.test/demo/1.gif')), true);
  assert.equal(await page.locator('.expanded-thumb,#image-hover').count(), 0);
  assert.equal(await page.evaluate(() => clickImage(1)), true);
  assert.equal(await page.locator('.expanded-thumb').count(), 0);
  assert.equal(requests.filter(url => url.endsWith('/1.gif')).length, 1);
});

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
  const filename = 'Owned Î» & <label>.png';
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
    document.getElementById('f1').dataset.imageFilename = 'Ã©'.repeat(128);
    imageConfig = { revealSpoilers: true }; imageController.refresh();
  });
  assert.equal(await page.locator('#f1 > p > a').count(), 1);
  assert.equal(await page.locator('#f1 > a.fileThumb').count(), 0);
});

test('modern spoiler metadata survives local copying and the owned body popup reveals and cleans up', async t => {
  const { page } = await fixture(t, { preview: true });
  await page.evaluate(() => {
    const file = document.getElementById('f1');
    file.replaceChildren();
    Object.assign(file.dataset, { imageSpoiler: 'true', imageFilename: 'Owned.png', thumbnailWidth: '10', thumbnailHeight: '10' });
    const header = document.createElement('div'); header.id = 'fT1'; header.className = 'fileText'; header.title = 'Owned.png';
    const label = document.createElement('a'); label.href = 'https://media.test/demo/1.png'; label.rel = 'noopener noreferrer'; label.textContent = 'Spoiler Image';
    header.append('File: ', label, ' (1 KB, 10x10)');
    const thumb = document.createElement('a'); thumb.className = 'fileThumb imgspoiler'; thumb.href = label.href; thumb.rel = label.rel;
    const img = document.createElement('img'); img.setAttribute('src', '/static/catalog/spoiler.png'); img.alt = '1 KB'; img.width = img.height = 100; img.loading = 'lazy';
    const caption = document.createElement('div'); caption.className = 'mFileInfo mobile'; caption.textContent = '1 KB PNG';
    thumb.append(img, caption); file.append(header, thumb);
    const context = { origin: 'https://boards.test', mediaOrigin: 'https://media.test', board: 'demo', thread: '1' };
    const tree = quotes.localQuoteTree(document.getElementById('pc1'), context, '1', imageOwnership);
    const popup = quotes.prepareQuotePost(tree, context, '1').build(document); popup.id = 'quote-preview'; popup.classList.add('preview'); document.body.append(popup);
    imageConfig = { revealSpoilers: true }; imageController.refresh();
    window.modernContext = context;
  });
  await page.waitForSelector('#quote-preview .nativeSpoilerRevealed');
  assert.equal(await page.locator('#quote-preview .fileThumb:not(.imgspoiler) img').getAttribute('src'), 'https://media.test/demo/1s.jpg');
  assert.equal(await page.locator('#quote-preview .fileText a').nth(1).innerText(), 'Owned.png');
  assert.equal(await page.evaluate(() => JSON.stringify(quotes.localQuoteTree(document.getElementById('pc1'), modernContext, '1', imageOwnership)).includes('data-image-filename')), true);
  await page.evaluate(() => { document.getElementById('quote-preview').remove(); imageController.refresh(); imageConfig = {}; imageController.refresh(); });
  assert.equal(await page.locator('#f1 .fileThumb:not(.imgspoiler)').count(), 0);
  assert.equal(await page.locator('#f1 .imgspoiler img').getAttribute('src'), '/static/catalog/spoiler.png');
});

test('retired exact file URLs close pending copies and prevent preview/inline reopening without blocking other images', async t => {
  const { page, modes, held, requests } = await fixture(t, { preview: true, config: { imageHover: true } });
  modes.set('/demo/1.png', 'hold');
  await page.evaluate(() => {
    const clone = document.getElementById('pc1').cloneNode(true); clone.id = 'quote-preview';
    document.body.append(clone);
    clone.querySelector('.fileThumb').dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
  });
  await page.waitForSelector('#image-hover', { state: 'attached' });
  assert.equal(await page.evaluate(() => imageController.retire('https://media.test/demo/1.png')), true);
  assert.equal(await page.locator('#image-hover,.expanded-thumb').count(), 0);
  const before = requests.filter(url => url.endsWith('/1.png')).length;
  assert.equal(await page.evaluate(() => {
    const popup = document.getElementById('quote-preview');
    popup.querySelector('.fileThumb').dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    return clickImage(1);
  }), true);
  assert.equal(await page.evaluate(() => imageController.retire('https://media.test/demo/1.png')), true);
  assert.deepEqual(await page.evaluate(() => {
    const popup = document.getElementById('quote-preview');
    return [['.file p a', 'click'], ['.fileThumb', 'auxclick']].map(([selector, type]) => {
      const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: type === 'auxclick' ? 1 : 0 });
      popup.querySelector(selector).dispatchEvent(event); return event.defaultPrevented;
    });
  }), [true, true]);
  for (const url of ['https://foreign.test/demo/2.png', 'https://media.test/demo/', 'https://media.test/demo/2.png?secret=x']) {
    assert.equal(await page.evaluate(url => imageController.retire(url), url), false);
  }
  for (const route of held) await route.fulfill({ contentType: 'image/png', body: pixel }).catch(() => {});
  assert.equal(await page.locator('#image-hover,.expanded-thumb').count(), 0);
  assert.equal(requests.filter(url => url.endsWith('/1.png')).length, before);
  assert.equal(await page.evaluate(() => clickImage(2)), true);
  await page.waitForSelector('#f2 .expanded-thumb:not([hidden])');
});

test('retirement capacity never evicts a deleted URL or permits late media loads', async t => {
  const { page } = await fixture(t, { limits: { retired: 1 } });
  assert.equal(await page.evaluate(() => imageController.retire('https://media.test/demo/1.png')), true);
  assert.equal(await page.evaluate(() => imageController.retire('https://media.test/demo/2.png')), false);
  assert.match(await page.locator('.nativeImageFeedback').textContent(), /Refresh this page/);
  for (const id of [1, 2, 3]) assert.equal(await page.evaluate(id => clickImage(id), id), true);
  assert.equal(await page.locator('.expanded-thumb,#image-hover').count(), 0);
});

test('PDF thumbnail keeps ordinary document navigation without image hover or expansion', async t => {
  const { page, requests } = await fixture(t, { pdf: true, count: 1,
    config: { imageExpansion: true, imageHover: true } });
  assert.equal(await page.evaluate(() => clickImage(1)), false);
  await page.locator('#f1 .fileThumb').hover();
  await page.waitForTimeout(100);
  assert.equal(requests.some(url => url.endsWith('.pdf')), false);
  assert.equal(await page.locator('.expandedImage,.nativeImageHover').count(), 0);
  assert.equal(await page.locator('#f1 .fileThumb').getAttribute('href'), 'https://media.test/demo/1.pdf');
  assert.equal(await page.evaluate(() => imageController.retire('https://media.test/demo/1.pdf')), true);
  assert.equal(await page.evaluate(() => clickImage(1)), true);
});


for (const video of ['red.webm', 'red-opus.webm', 'blue.mp4', 'blue-aac.mp4']) {
  test(`owned ${video} expands with native controls and releases media on close and deletion`, async t => {
    const { page, requests } = await fixture(t, { video, count: 1 });
    assert.equal(await page.evaluate(() => clickImage(1)), true);
    const player = page.locator('#f1 video.expanded-thumb:not([hidden])');
    await player.waitFor();
    assert.deepEqual(await player.evaluate(v => [v.controls, v.loop, v.muted, v.playsInline, v.videoWidth, v.videoHeight]),
      [true, true, true, true, 64, 48]);
    assert.equal(await page.locator('#f1 a.fileThumb').isVisible(), false);
    await page.evaluate(() => { window.retainedVideo = document.querySelector('#f1 video'); retainedVideo.volume = 0.25; });
    await page.locator('#f1 .nativeVideoClose').click();
    assert.deepEqual(await page.evaluate(() => [retainedVideo.isConnected, retainedVideo.paused, retainedVideo.getAttribute('src')]), [false, true, null]);
    assert.equal(await page.locator('#f1 a.fileThumb').isVisible(), true);
    assert.equal(await page.evaluate(() => clickImage(1)), true);
    await player.waitFor();
    await page.evaluate(() => imageController.retire(document.querySelector('#f1 a.fileThumb').href));
    assert.equal(await page.locator('#f1 video').count(), 0);
    const before = requests.length;
    await page.evaluate(() => clickImage(1));
    await page.waitForTimeout(30);
    assert.equal(requests.length, before);
  });
}

test('video hover reports duration and tears down on settings disable', async t => {
  const { page } = await fixture(t, { video: 'red.webm', count: 1, config: { imageHover: true } });
  await page.locator('#f1 a.fileThumb').hover();
  await page.locator('video#image-hover:not([hidden])').waitFor();
  assert.equal(await page.locator('.nativeVideoDuration').textContent(), '0:00');
  await page.evaluate(() => {
    window.retainedVideo = document.querySelector('video#image-hover');
    imageConfig.disableAll = true; imageController.refresh();
  });
  assert.deepEqual(await page.evaluate(() => [retainedVideo.isConnected, retainedVideo.paused, retainedVideo.getAttribute('src')]), [false, true, null]);
  assert.equal(await page.locator('.nativeVideoDuration').count(), 0);
});


test('video volume, mobile sizing, offscreen pause and pagehide release use the native player', async t => {
  const { page } = await fixture(t, { video: 'red.webm', count: 1, storageOrigin: true, config: { unmuteWebm: true } });
  await page.evaluate(() => localStorage.setItem('4chan-volume','0.25'));
  await page.setViewportSize({ width: 390, height: 700 });
  await page.evaluate(() => clickImage(1));
  const player = page.locator('#f1 video:not([hidden])');
  await player.waitFor();
  assert.deepEqual(await player.evaluate(v => [v.muted,v.volume,v.playsInline]), [false,0.25,true]);
  assert.ok((await player.boundingBox()).width <= 390);
  await player.evaluate(v => { v.volume=0.75; });
  await page.waitForFunction(() => localStorage.getItem('4chan-volume') === '0.75');
  const projection = await page.evaluate(() => {
    const tree=quotes.localQuoteTree(document.getElementById('pc1'),
      {origin:'https://boards.test',mediaOrigin:'https://media.test',board:'demo',thread:'1'},'1',imageOwnership);
    return JSON.stringify(tree);
  });
  assert.ok(!projection.includes('nativeVideoClose') && !projection.includes('expanded-thumb'));
  await page.evaluate(async () => {
    const v=document.querySelector('#f1 video'); v.muted=true; await v.play();
    document.body.style.height='3000px'; window.scrollTo(0,2000);
  });
  await page.waitForFunction(() => document.querySelector('#f1 video').paused);
  await page.evaluate(() => { window.retainedVideo=document.querySelector('#f1 video'); window.dispatchEvent(new PageTransitionEvent('pagehide')); });
  assert.deepEqual(await page.evaluate(() => [retainedVideo.isConnected,retainedVideo.paused,retainedVideo.getAttribute('src')]), [false,true,null]);
});

test('video failure restores the thumbnail and preserves the filename fallback', async t => {
  const { page, modes } = await fixture(t, { video: 'red.webm', count: 1 });
  modes.set('/demo/1.webm','fail');
  await page.evaluate(() => clickImage(1));
  await page.locator('.nativeImageFeedback').waitFor();
  assert.match(await page.locator('.nativeImageFeedback').textContent(), /filename link/);
  assert.equal(await page.locator('#f1 video').count(),0);
  assert.equal(await page.locator('#f1 a.fileThumb').isVisible(),true);
  assert.equal(await page.locator('#f1 p a').getAttribute('href'),'https://media.test/demo/1.webm');
});

for (const video of ['red.webm', 'red-opus.webm', 'red-vp9.webm']) {
  test(`pinned fallback decodes ${video}, exposes controls and stops on close`, async t => {
    const { page, requests } = await fixture(t, { video, fallback: true, storageOrigin: true, count: 1 });
    await page.evaluate(() => clickImage(1));
    const player = page.locator('#f1 .expanded-thumb:not([hidden])');
    await player.waitFor();
    assert.deepEqual(await player.evaluate(v => [v.videoWidth, v.videoHeight, v.muted]), [64, 48, true]);
    assert.ok(await player.evaluate(v => v.querySelector('canvas') !== null));
    await player.evaluate(v => { window.fallbackEnds = 0; v.addEventListener('ended', () => { fallbackEnds++; }); });
    await page.waitForFunction(() => window.fallbackEnds >= 2);
    await page.locator('.nativeVideoControls button', { hasText: /^Pause$/ }).click();
    assert.equal(await player.evaluate(v => v.paused), true);
    await page.locator('.nativeVideoControls button', { hasText: /^Unmute$/ }).click();
    assert.equal(await player.evaluate(v => v.muted), false);
    await page.locator('.nativeVideoControls input[aria-label="Video volume"]').evaluate(node => { node.value = '0.25'; node.dispatchEvent(new Event('input', { bubbles: true })); });
    await page.waitForTimeout(250);
    assert.equal(await page.evaluate(() => localStorage.getItem('4chan-volume')), '0.25');
    await page.evaluate(() => { window.retainedFallback = document.querySelector('#f1 .expanded-thumb'); });
    await page.locator('.nativeVideoClose').click();
    assert.equal(await page.locator('.expanded-thumb,.nativeVideoControls').count(), 0);
    assert.equal(await page.evaluate(() => retainedFallback.paused), true);
    assert.ok(requests.some(url => url.includes('ogv-demuxer-webm-wasm.wasm')));
    assert.ok(requests.some(url => url.includes(video === 'red-vp9.webm' ? 'ogv-decoder-video-vp9-wasm.wasm' : 'ogv-decoder-video-vp8-wasm.wasm')));
    if (video !== 'red.webm') assert.ok(requests.some(url => url.includes(video === 'red-vp9.webm' ? 'ogv-decoder-audio-vorbis-wasm.wasm' : 'ogv-decoder-audio-opus-wasm.wasm')));
  });
}

test('fallback cancellation does not start a late decoder or leave controls', async t => {
  const { page, requests } = await fixture(t, { video: 'red.webm', fallback: true, storageOrigin: true, count: 1 });
  let held;
  await page.route('**/static/ogv/ogv.js', route => { held = route; });
  await page.evaluate(() => clickImage(1));
  await assertEventually(() => held !== undefined);
  await page.locator('.nativeVideoClose').click();
  await held.fulfill({ contentType: 'text/javascript', body: await readFile(new URL('apps/public/static/ogv/ogv.js', root)) });
  await page.waitForTimeout(200);
  assert.equal(await page.locator('.expanded-thumb,.nativeVideoControls').count(), 0);
  assert.equal(requests.some(url => /\/1\.webm$/.test(url)), false);
});

async function assertEventually(predicate) {
  for (let i = 0; i < 100; i++) {
    if (predicate()) return;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.ok(predicate(), 'owned deferred request was not reached');
}
