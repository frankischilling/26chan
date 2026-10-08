import test, { before, after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

let browser;
before(async () => { browser = await chromium.launch({ headless: true }); });
after(async () => { await browser?.close(); });
const root = new URL('../../', import.meta.url);
const core = await readFile(new URL('apps/public/static/thread-watcher-core.v1.js', root), 'utf8');
const source = (await readFile(new URL('apps/public/client/native-post-deletion.js', root), 'utf8'))
  .replace('../static/thread-watcher-core.v1.js', `data:text/javascript;base64,${Buffer.from(core).toString('base64')}`);
const imageSource = await readFile(new URL('apps/public/static/native-images.v1.js', root), 'utf8');
const css = await readFile(new URL('apps/public/static/board.css', root), 'utf8');
const id = '9223372036854775807';
const pixel = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==', 'base64');

async function fixture(t, { images = false } = {}) {
  const context = await browser.newContext({ viewport: { width: 400, height: 800 } });
  t.after(() => context.close());
  const page = await context.newPage();
  const requests = [];
  await page.route('**/*', async route => {
    requests.push(route.request().url());
    if (route.request().url().startsWith('https://media.test/')) {
      await route.fulfill({ contentType: 'image/png', body: pixel }); return;
    }
    await route.fulfill({ contentType: 'text/html', body: '<!doctype html><html><body></body></html>' });
  });
  await page.goto('https://boards.test/demo/thread/1');
  await page.addStyleTag({ content: css });
  await page.evaluate(async ({ source, imageSource, id, images }) => {
    const module = async text => {
      const url = URL.createObjectURL(new Blob([text], { type: 'text/javascript' }));
      try { return await import(url); } finally { URL.revokeObjectURL(url); }
    };
    const deletion = await module(source);
    document.body.innerHTML = `<main class="board"><section class="thread" id="t1" data-archived="false">
      <article class="postContainer replyContainer" id="pc${id}"><div class="post reply" id="p${id}">
      <div class="file" id="f${id}"><div class="fileText">File: <a href="https://media.test/demo/3.png">owned.png</a></div>
      <a class="fileThumb" href="https://media.test/demo/3.png"><img src="https://media.test/demo/3s.jpg" width="10" height="10"></a></div>
      <blockquote class="postMessage">Original content</blockquote><details class="postActions"><summary>Delete or report</summary>
      <form action="/demo/delete" method="post"><input type="hidden" name="no" value="${id}">
      <input type="hidden" name="password" value="unchanged-password"><input type="checkbox" name="file_only" value="true"><button>Delete post</button></form>
      </details></div></article></section></main>`;
    window.config = { imageExpansion: true, imageHover: true }; window.mobile = true; window.confirmed = true;
    window.prompts = []; window.calls = []; window.savedPost = document.getElementById(`p${id}`);
    window.savedContainer = document.getElementById(`pc${id}`); window.savedImage = document.querySelector('.fileThumb > img');
    window.savedForm = document.querySelector('form'); window.originalForm = savedForm.outerHTML;
    window.sendDeletion = options => new Promise((resolve, reject) => { calls.push({ options, resolve, reject }); });
    const board = document.querySelector('.board');
    window.imageController = images ? (await module(imageSource)).mountNativeImages({ root: board,
      mediaOrigin: 'https://media.test', settings: () => config }) : null;
    window.deletionController = deletion.mountNativeDeletion({ root: board, board: 'demo', settings: () => config,
      mobileLayout: () => mobile, images: imageController, confirm: text => { prompts.push(text); return confirmed; },
      send: sendDeletion });
    window.beginDelete = fileOnly => { window.lastDeletion = deletionController.remove(savedPost, fileOnly); };
    window.completeDelete = (value = true) => { calls.at(-1).resolve(value); return lastDeletion; };
    window.formSubmit = () => {
      const event = new Event('submit', { bubbles: true, cancelable: true }); savedForm.dispatchEvent(event); return event.defaultPrevented;
    };
    window.clickImage = () => {
      const event = new MouseEvent('click', { bubbles: true, cancelable: true });
      const stopNavigation = event => event.preventDefault();
      document.addEventListener('click', stopNavigation, { once: true });
      document.querySelector('.fileThumb').dispatchEvent(event);
    };
  }, { source, imageSource, id, images });
  return { page, requests };
}

test('canceling is inert; confirmed post deletion retains and dims the exact original container', async t => {
  const { page } = await fixture(t);
  assert.equal(await page.evaluate(() => { confirmed = false; return deletionController.remove(savedPost); }), false);
  assert.deepEqual(await page.evaluate(() => ({ prompts, calls: calls.length, form: savedForm.outerHTML === originalForm,
    fallbackPrevented: formSubmit() })), { prompts: ['Delete post?'], calls: 0, form: true, fallbackPrevented: false });
  await page.evaluate(() => { confirmed = true; beginDelete(false); });
  assert.equal(await page.evaluate(() => completeDelete()), true);
  assert.deepEqual(await page.evaluate(() => ({ exact: document.getElementById(savedContainer.id) === savedContainer,
    deleted: savedContainer.classList.contains('deleted'), opacity: getComputedStyle(savedContainer).opacity,
    text: savedPost.querySelector('.postMessage').textContent, busy: savedPost.hasAttribute('aria-busy'),
    fields: savedForm.outerHTML === originalForm, again: deletionController.canDelete(savedPost), fallback: formSubmit() })),
  { exact: true, deleted: true, opacity: '0.66', text: 'Original content', busy: false, fields: true, again: false, fallback: true });
});

test('file deletion marks the original thumbnail, contracts images and never reopens deleted media', async t => {
  const { page, requests } = await fixture(t, { images: true });
  await page.waitForFunction(() => savedImage.complete && savedImage.naturalWidth > 0);
  await page.evaluate(() => clickImage());
  await page.waitForSelector('.expanded-thumb:not([hidden])');
  await page.evaluate(() => beginDelete(true));
  assert.equal(await page.evaluate(() => completeDelete()), true);
  const before = requests.filter(url => url.endsWith('/3.png')).length;
  await page.evaluate(() => {
    clickImage(); document.querySelector('.fileThumb').dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
  });
  assert.deepEqual(await page.evaluate(() => ({ original: document.querySelector('.fileThumb > img') === savedImage,
    file: savedImage.closest('.file').classList.contains('deleted'), thumbnail: savedImage.classList.contains('deleted'),
    thumbnailOpacity: getComputedStyle(savedImage).opacity, fileOpacity: getComputedStyle(savedImage.closest('.file')).opacity,
    postDeleted: savedContainer.classList.contains('deleted'), expanded: !!document.querySelector('.expanded-thumb,#image-hover'),
    canPost: deletionController.canDelete(savedPost), canFile: deletionController.canDelete(savedPost, true), prompts })),
  { original: true, file: true, thumbnail: true, thumbnailOpacity: '0.66', fileOpacity: '1', postDeleted: false,
    expanded: false, canPost: true, canFile: false, prompts: ['Delete file?'] });
  assert.equal(requests.filter(url => url.endsWith('/3.png')).length, before);
  assert.equal(await page.evaluate(() => {
    savedForm.elements.file_only.checked = true; return formSubmit();
  }), true);
  assert.equal(await page.evaluate(() => {
    savedForm.elements.file_only.checked = false; return formSubmit();
  }), false);
});

test('pending and unknown requests block duplicate post/file menu and ordinary-form submissions', async t => {
  const { page } = await fixture(t);
  await page.evaluate(() => beginDelete(true));
  assert.equal(await page.evaluate(() => deletionController.remove(savedPost)), false);
  assert.equal(await page.evaluate(() => formSubmit()), true);
  assert.deepEqual(await page.evaluate(() => ({ count: calls.length, prompts })), { count: 1, prompts: ['Delete file?'] });
  assert.equal(await page.evaluate(() => completeDelete(false)), false);
  assert.match(await page.locator('.nativeDeletionFeedback').textContent(), /could not be confirmed.*Refresh the page/);
  assert.equal(await page.evaluate(() => deletionController.remove(savedPost, true)), false);
  assert.equal(await page.evaluate(() => deletionController.remove(savedPost)), false);
  assert.equal(await page.evaluate(() => formSubmit()), true);
  assert.equal(await page.evaluate(() => calls.length), 1);
  assert.equal(await page.locator('.deleted').count(), 0);
});

test('cloned and whole-board replacement fallback forms cannot duplicate tracked submissions', async t => {
  const { page } = await fixture(t);
  await page.evaluate(() => beginDelete(false));
  assert.equal(await page.evaluate(() => {
    const clone = savedContainer.cloneNode(true); savedContainer.after(clone);
    const event = new Event('submit', { bubbles: true, cancelable: true });
    clone.querySelector('form').dispatchEvent(event); return event.defaultPrevented;
  }), true);
  await page.evaluate(() => {
    const board = document.querySelector('.board'); board.replaceWith(board.cloneNode(true));
  });
  await page.waitForFunction(() => calls[0].options.signal.aborted);
  assert.equal(await page.evaluate(() => {
    const event = new Event('submit', { bubbles: true, cancelable: true });
    document.querySelector('form').dispatchEvent(event); return event.defaultPrevented;
  }), true);
  assert.equal(await page.evaluate(() => completeDelete()), false);
  assert.equal(await page.evaluate(() => calls.length), 1);
});

test('a known rejection renders plain feedback and permits an explicit corrected retry', async t => {
  const { page } = await fixture(t);
  await page.evaluate(() => beginDelete(false));
  assert.equal(await page.evaluate(async () => {
    const error = new Error('<img src=x onerror=alert(1)>'); error.deletionOutcome = 'rejected';
    calls[0].reject(error); return lastDeletion;
  }), false);
  assert.match(await page.locator('.nativeDeletionFeedback').textContent(), /Deletion was rejected/);
  assert.equal(await page.locator('.nativeDeletionFeedback img').count(), 0);
  assert.equal(await page.evaluate(() => formSubmit()), false);
  await page.evaluate(() => beginDelete(false));
  assert.equal(await page.evaluate(() => calls.length), 2);
  assert.equal(await page.evaluate(() => completeDelete()), true);
});

for (const change of ['replace-post', 'replace-file', 'replace-form', 'replace-board', 'disable', 'archive', 'pagehide']) {
  test(`${change} aborts and fences a pending deletion before late success`, async t => {
    const { page } = await fixture(t);
    await page.evaluate(() => beginDelete(true));
    await page.evaluate(change => {
      if (change === 'replace-post') savedPost.replaceWith(savedPost.cloneNode(true));
      if (change === 'replace-file') savedImage.closest('.file').replaceWith(savedImage.closest('.file').cloneNode(true));
      if (change === 'replace-form') savedForm.replaceWith(savedForm.cloneNode(true));
      if (change === 'replace-board') document.querySelector('.board').replaceWith(document.querySelector('.board').cloneNode(true));
      if (change === 'disable') { config.disableAll = true; document.dispatchEvent(new Event('4chanSettingsSaved')); }
      if (change === 'archive') document.querySelector('.thread').dataset.archived = 'true';
      if (change === 'pagehide') window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
    }, change);
    await page.waitForFunction(() => calls[0].options.signal.aborted);
    assert.equal(await page.evaluate(() => completeDelete()), false);
    assert.equal(await page.locator('.deleted').count(), 0);
    assert.equal(await page.evaluate(() => savedImage.classList.contains('deleted')), false);
    assert.equal(await page.evaluate(() => savedPost.hasAttribute('aria-busy')), false);
    if (change === 'pagehide') {
      await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
      assert.match(await page.locator('.nativeDeletionFeedback').textContent(), /Refresh the page/);
      assert.equal(await page.evaluate(() => deletionController.remove(savedPost)), false);
      assert.equal(await page.evaluate(() => calls.length), 1);
    }
  });
}

test('desktop, never-mobile, archived, duplicate-id and projected post targets have no deletion authority', async t => {
  const { page } = await fixture(t);
  assert.deepEqual(await page.evaluate(async () => {
    const results = [];
    mobile = false; results.push(await deletionController.remove(savedPost)); mobile = true;
    document.querySelector('.thread').dataset.archived = 'true'; results.push(await deletionController.remove(savedPost));
    document.querySelector('.thread').dataset.archived = 'false';
    const clone = savedContainer.cloneNode(true); savedContainer.after(clone); results.push(await deletionController.remove(clone.querySelector('.post')));
    savedForm.elements.no.value = '9007199254740992'; results.push(await deletionController.remove(savedPost));
    return { results, calls: calls.length, prompts };
  }), { results: [false, false, false, false], calls: 0, prompts: [] });
});
