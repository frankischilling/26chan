import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

const origin = 'https://update.example';
const post = id => `<article class="postContainer ${id === '100' ? 'op' : 'reply'}Container" id="pc${id}"><div class="post ${id === '100' ? 'op' : 'reply'}" id="p${id}"><div class="postInfo" id="pi${id}"><span class="name">Anonymous</span><time datetime="2026-01-01T00:00:00Z">Owned date</time><span class="postNum"><a href="/test/thread/100#p${id}" title="Link to this post">No.</a><a href="/test/thread/100?quote=${id}#reply" title="Reply to this post">${id}</a></span></div><blockquote class="postMessage" id="m${id}">Owned ${id}</blockquote></div></article>`;
const snapshot = { version: 2, board: 'test', thread: '100', replies: 2, images: 0,
  closed: true, sticky: true, archived: false, tail_size: 1, tail_id: null,
  posts: ['100', '101', '102'].map(no => ({ no, html: post(no), file_deleted: false })) };

test('updater application commits only a complete successful page integration', async t => {
  const files = {};
  for (const name of ['native-thread-controls.v1.js', 'native-filter.v1.js']) {
    files[`/static/${name}`] = await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8');
  }
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup(mode) {
      const context = await browser.newContext(), page = await context.newPage();
      await page.route('**/*', route => {
        const path = new URL(route.request().url()).pathname;
        if (files[path]) return route.fulfill({ contentType: 'text/javascript', body: files[path] });
        if (path === '/_watch/test/thread/100/posts') return route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) });
        if (path === '/test/thread/100') return route.fulfill({ contentType: 'text/html', body:
          `<!doctype html><title>Owned updater</title><nav class="threadNav desktop"></nav><form class="postEditor"><textarea id="draft">owned draft</textarea><button>Post</button></form><main class="board"><section class="thread" id="t100" data-tail-size="0" data-sticky="false" data-closed="false" data-archived="false">${post('100')}${post('101')}</section></main>` });
        return route.abort();
      });
      await page.goto(`${origin}/test/thread/100`);
      await page.evaluate(async mode => {
        window.config = {}; window.events = []; window.mode = mode; window.integrations = 0;
        for (const name of ['boardThreadStateChanged', '4chanThreadUpdated']) document.addEventListener(name, () => events.push(name));
        const { mountNativeThreadUpdater } = await import('/static/native-thread-controls.v1.js');
        window.updater = mountNativeThreadUpdater({ board: 'test', thread: '100', worksafe: true, mediaOrigin: '', settings: () => config,
          applied: async (_snapshot, signal) => {
            integrations++;
            if (window.mode === 'fail') throw new Error('owned filter rejection');
            if (window.mode === 'hold') await new Promise(resolve => { window.complete = resolve; window.heldSignal = signal; });
          },
        });
      }, mode);
      const state = () => page.evaluate(() => ({
        ids: [...document.querySelectorAll('.postContainer')].map(node => node.id),
        tail: document.querySelector('.thread').dataset.tailSize,
        closed: document.querySelector('.thread').dataset.closed,
        sticky: document.querySelector('.thread').dataset.sticky,
        disabled: document.querySelector('#draft').disabled,
        draft: document.querySelector('#draft').value, events,
      }));
      return { context, page, state };
    }
    const original = { ids: ['pc100', 'pc101'], tail: '0', closed: 'false', sticky: 'false', disabled: false, draft: 'owned draft', events: [] };
    await t.test('a rejected callback restores posts and retains original metadata, forms and event state', async () => {
      const { context, page, state } = await setup('fail');
      try {
        await page.evaluate(() => updater.update());
        assert.deepEqual(await state(), original);
        assert.match(await page.locator('.nativeUpdaterStatus').textContent(), /could not be applied/);
      } finally { await context.close(); }
    });
    await t.test('cancellation removes temporary additions before an uncooperative callback returns', async () => {
      const { context, page, state } = await setup('hold');
      try {
        await page.evaluate(() => { window.pending = updater.update(); });
        await page.waitForFunction(() => typeof complete === 'function');
        await page.evaluate(() => { config.threadUpdater = false; updater.sync(); });
        assert.equal(await page.evaluate(() => heldSignal.aborted), true);
        assert.deepEqual(await state(), original);
        await page.evaluate(async () => { complete(); await pending; });
        assert.deepEqual(await state(), original);
      } finally { await context.close(); }
    });
    await t.test('a successful callback commits the new metadata and emits each public event once', async () => {
      const { context, page, state } = await setup('pass');
      try {
        await page.evaluate(() => updater.update());
        assert.deepEqual(await state(), { ...original, ids: ['pc100', 'pc101', 'pc102'], tail: '1', closed: 'true', sticky: 'true', disabled: true,
          events: ['boardThreadStateChanged', '4chanThreadUpdated'] });
        assert.equal(await page.evaluate(() => integrations), 1);
      } finally { await context.close(); }
    });
  } finally { await browser.close(); }
});
