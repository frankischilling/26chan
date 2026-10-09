import { test as base, expect } from '@playwright/test';
import { ownedDeletionMarker } from './helpers/deletion-fixture.js';
import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { observeOwnedDeletionResponse } from './owned-upload-response.mjs';

const origin = 'http://127.0.0.1:3000';
const endpoint = `${origin}/fixture/imgboard.php`;
const password = 'owned-native-deletion-password';
const headers = { Origin: origin, Connection: 'close' };
const unknown = 'Deletion could not be confirmed. Refresh the page before trying again.';
const trigger = (page, id) => page.getByRole('button', { name: `Post menu for post ${id}`, exact: true });
const feedback = page => page.locator('.nativeDeletionFeedback');

// Posting through this browser context establishes actual server-owned anonymous
// authority. A tracking receipt, DOM marker or deletion password is not used by
// the mobile action. The independent request fixture is only for reads/cleanup.
const test = base.extend({
  owned: async ({ context, request }, use) => {
    const marker = ownedDeletionMarker();
    let id;
    async function write(resto, com) {
      const response = await withPostingHistory(() => context.request.post(`${origin}/fixture/post`, {
        headers, maxRedirects: 0, form: { resto, com, sub: resto === '0' ? marker : '', password },
      }));
      expect(response.status(), 'Owned fixture must persist through the real server').toBe(303);
      const receipt = /\/fixture\/thread\/([1-9][0-9]*)#p([1-9][0-9]*)$/.exec(response.headers().location);
      expect(receipt, 'Use the exact server receipt, never a predicted post ID').not.toBeNull();
      expect(receipt[1]).toBe(resto === '0' ? receipt[2] : resto);
      return receipt[2];
    }
    try {
      id = await write('0', 'Owned mobile deletion OP');
      const reply = await write(id, 'Owned mobile deletion reply');
      const keep = await write(id, 'Owned mobile deletion control reply');
      await use({ id, reply, keep, url: `/fixture/thread/${id}` });
    } finally {
      if (id) {
        const remaining = await request.get(`/fixture/thread/${id}.json`);
        expect([200, 404]).toContain(remaining.status());
        if (remaining.status() === 200) await withDeletionQuota(async () => {
          const removed = await request.post('/fixture/delete', {
            headers, maxRedirects: 0, form: { no: id, password },
          });
          expect(removed.status()).toBe(303);
          expect((await request.get(`/fixture/thread/${id}.json`)).status()).toBe(404);
        });
      }
    }
  },
});

test.use({ viewport: { width: 390, height: 844 }, trace: 'off' });

async function open(page, owned, settings = {}) {
  await page.addInitScript(settings => {
    localStorage.setItem('4chan-settings', JSON.stringify({
      threadWatcher: false, threadUpdater: false, threadStats: false, ...settings,
    }));
  }, settings);
  await page.goto(new URL(owned.url, origin).href);
  await expect(trigger(page, owned.reply)).toBeVisible();
}

async function choose(page, id, accept) {
  await trigger(page, id).click();
  const dialogReady = page.waitForEvent('dialog');
  const clicked = page.getByRole('menuitem', { name: 'Delete post', exact: true }).click();
  const dialog = await dialogReady;
  const message = dialog.message(), type = dialog.type();
  if (accept) await dialog.accept(); else await dialog.dismiss();
  await clicked;
  expect(type).toBe('confirm');
  expect(message).toBe('Delete post?');
}

function writes(page) {
  const observed = [];
  page.on('request', request => {
    if (request.method() === 'POST' && [endpoint, `${origin}/fixture/delete`].includes(request.url())) observed.push(request);
  });
  return observed;
}

function expectNativeRequest(request, id) {
  expect(request.url()).toBe(endpoint);
  expect(request.method()).toBe('POST');
  expect(request.isNavigationRequest()).toBe(false);
  expect([...new URLSearchParams(request.postData())].sort()).toEqual([[id, 'delete'], ['mode', 'usrdel']].sort());
}

async function ids(request, owned) {
  const response = await request.get(`${origin}${owned.url}.json`);
  expect(response.status()).toBe(200);
  return (await response.json()).posts.map(post => String(post.no));
}

async function rememberDocument(page, id) {
  await page.evaluate(id => {
    window.ownedDeletionDocument = document;
    window.ownedDeletionPost = document.getElementById(`p${id}`);
    window.ownedDeletionHistory = history.length;
  }, id);
}

async function expectSameDocument(page, owned, id) {
  await expect(page).toHaveURL(`${origin}${owned.url}`);
  expect(await page.evaluate(id => document === window.ownedDeletionDocument
    && document.getElementById(`p${id}`) === window.ownedDeletionPost
    && history.length === window.ownedDeletionHistory, id)).toBe(true);
}

// Delay only delivery of a real server response. No deletion result or server
// state is fabricated, and route.fetch never retries this mutation.
async function holdDeletion(page) {
  let release, captured, failed, complete, active = false, closed = false;
  const gate = new Promise(resolve => { release = resolve; });
  const ready = new Promise((resolve, reject) => { captured = resolve; failed = reject; });
  const finished = new Promise(resolve => { complete = resolve; });
  const handler = async route => {
    active = true;
    try {
      const response = await route.fetch({ maxRedirects: 0, maxRetries: 0 });
      const body = await response.text();
      captured({ status: response.status(), body, request: route.request() });
      await gate;
      // A fenced operation aborts its browser fetch before the held response is
      // released. The successful server mutation above remains authoritative.
      await route.fulfill({ response, body }).catch(error => {
        if (!route.request().failure()) throw error;
      });
    } catch (error) { failed(error); throw error; }
    finally { complete(); }
  };
  await page.route(endpoint, handler, { times: 1 });
  return { ready, release, async close() {
    if (closed) return;
    closed = true; release();
    await page.unroute(endpoint, handler);
    if (active) await finished;
  } };
}

test('canceling OP and reply confirmation preserves form state, document and actual server posts', async ({ page, request, owned }) => {
  await open(page, owned);
  await rememberDocument(page, owned.reply);
  const observed = writes(page), before = await ids(request, owned);
  await page.locator(`#p${owned.reply} .postActions > summary`).click();
  await page.locator(`#delete${owned.reply}`).evaluate(input => { input.value = 'owned cancellation draft'; });
  for (const id of [owned.id, owned.reply]) {
    await choose(page, id, false);
    await expect(page.locator(`#pc${id}`)).not.toHaveClass(/\bdeleted\b/);
    await expect(page.locator(`#p${id}`)).not.toHaveAttribute('aria-busy');
  }
  await expect(page.locator(`#delete${owned.reply}`)).toHaveValue('owned cancellation draft');
  await expect(feedback(page)).toHaveCount(0);
  await expectSameDocument(page, owned, owned.reply);
  expect(await ids(request, owned)).toEqual(before);
  expect(observed).toHaveLength(0);
});

for (const target of ['reply', 'id']) {
  test(`owned ${target === 'id' ? 'OP' : 'reply'} deletion marks only its original container without navigation and persists on reload`, async ({ page, request, owned }) => {
    await open(page, owned);
    const id = owned[target], observed = writes(page);
    await rememberDocument(page, id);
    await page.locator(`#p${owned.keep} .postActions > summary`).click();
    await page.locator(`#delete${owned.keep}`).evaluate(input => { input.value = 'untouched control draft'; });
    await withDeletionQuota(async () => {
      // Capture the real body before native deletion settles and aborts its
      // fetch, which can discard Chromium's separate DevTools body copy.
      const deleted = await observeOwnedDeletionResponse(page, endpoint);
      const finished = page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'POST');
      await choose(page, id, true);
      const response = await finished;
      expect(response.status()).toBe(200);
      const captured = await deleted();
      expect(captured.status).toBe(response.status());
      expect(captured.text).toContain('The deletion was completed.');
      await expect(feedback(page)).toHaveText(`Post No.${id}: Post deleted.`);
      await expect(page.locator(`#pc${id}`)).toHaveClass(/\bdeleted\b/);
      await expect(page.locator(`#pc${id}`)).toHaveCSS('opacity', '0.66');
      await expect(page.locator(`#p${id}`)).not.toHaveAttribute('aria-busy');
      await expect(page.locator(`#pc${owned.keep}`)).not.toHaveClass(/\bdeleted\b/);
      await expect(page.locator(`#delete${owned.keep}`)).toHaveValue('untouched control draft');
      await expectSameDocument(page, owned, id);
      expect(observed).toHaveLength(1);
      expectNativeRequest(observed[0], id);
      if (target === 'id') {
        expect((await request.get(`${owned.url}.json`)).status()).toBe(404);
        expect((await page.reload()).status()).toBe(404);
      } else {
        expect(await ids(request, owned)).toEqual([owned.id, owned.keep]);
        await trigger(page, id).click();
        await expect(page.getByRole('menuitem', { name: 'Delete post', exact: true })).toHaveCount(0);
        await page.reload();
        await expect(page.locator(`#pc${id}`)).toHaveCount(0);
        await expect(page.locator(`#pc${owned.keep}`)).toBeVisible();
      }
    });
  });
}

test('a different browser session receives the real denial and cannot mark or delete the owned post', async ({ browser, request, owned }) => {
  const stranger = await browser.newContext({ viewport: { width: 390, height: 844 } });
  try {
    const page = await stranger.newPage();
    await open(page, owned);
    await rememberDocument(page, owned.reply);
    const observed = writes(page), held = await holdDeletion(page);
    try {
      await withDeletionQuota(async () => {
        await choose(page, owned.reply, true);
        // The client cancels its reader and aborts its fetch during settlement,
        // so Chromium may discard the inspector body before response.text().
        // Capture this real server response before delivering it unchanged.
        const captured = await held.ready;
        expect(captured.status).toBe(403);
        expect(captured.body).toContain('Password incorrect.');
        const finished = page.waitForResponse(response => response.url() === endpoint && response.request().method() === 'POST');
        held.release();
        const response = await finished;
        expect(response.status()).toBe(403);
        expect(response.request()).toBe(captured.request);
        await expect(feedback(page)).toHaveText(`Post No.${owned.reply}: Deletion was rejected. Refresh the page before trying again.`);
        await expect(page.locator(`#pc${owned.reply}`)).not.toHaveClass(/\bdeleted\b/);
        await expect(page.locator(`#p${owned.reply}`)).not.toHaveAttribute('aria-busy');
        await expectSameDocument(page, owned, owned.reply);
        expect(await ids(request, owned)).toEqual([owned.id, owned.reply, owned.keep]);
        expect(observed).toHaveLength(1);
        expectNativeRequest(observed[0], owned.reply);
      });
    } finally { await held.close(); }
  } finally { await stranger.close(); }
});

test('duplicate mobile and fallback-form clicks stay bounded while the actual deletion response is pending', async ({ page, request, owned }) => {
  await open(page, owned);
  await rememberDocument(page, owned.reply);
  const observed = writes(page), held = await holdDeletion(page);
  try {
    await withDeletionQuota(async () => {
      await choose(page, owned.reply, true);
      const response = await held.ready;
      expect(response.status).toBe(200);
      expect(response.body).toContain('The deletion was completed.');
      expect(await ids(request, owned)).toEqual([owned.id, owned.keep]);
      await expect(page.locator(`#p${owned.reply}`)).toHaveAttribute('aria-busy', 'true');
      await expect(page.locator(`#pc${owned.reply}`)).not.toHaveClass(/\bdeleted\b/);
      const dialogs = [];
      page.on('dialog', async dialog => { dialogs.push(dialog.message()); await dialog.dismiss(); });
      await trigger(page, owned.reply).click();
      await page.getByRole('menuitem', { name: 'Delete post', exact: true }).click();
      await expect(feedback(page)).toContainText('Deletion is already in progress.');
      await page.locator(`#p${owned.reply} form[action="/fixture/delete"]`).evaluate(form => form.requestSubmit());
      await expect(feedback(page)).toContainText('Deletion is already in progress.');
      expect(dialogs).toEqual([]);
      expect(observed).toHaveLength(1);
      expectNativeRequest(observed[0], owned.reply);
      // A menu reopened during the pending mutation must not retain stale
      // actions when its own target is marked deleted.
      await trigger(page, owned.reply).click();
      await expect(page.locator('#post-menu')).toBeVisible();
      held.release();
      await expect(feedback(page)).toHaveText(`Post No.${owned.reply}: Post deleted.`);
      await expect(page.locator(`#pc${owned.reply}`)).toHaveClass(/\bdeleted\b/);
      await expect(page.locator('#post-menu')).toHaveCount(0);
      await expectSameDocument(page, owned, owned.reply);
      expect(observed).toHaveLength(1);
    });
  } finally { await held.close(); }
});

for (const invalidation of ['disableAll', 'deletion-form replacement']) {
  test(`${invalidation} during a real deletion fences its delayed success and prevents a blind retry`, async ({ page, request, owned }) => {
    await open(page, owned);
    const observed = writes(page), held = await holdDeletion(page);
    try {
      await withDeletionQuota(async () => {
        await choose(page, owned.reply, true);
        expect((await held.ready).status).toBe(200);
        expect(await ids(request, owned)).toEqual([owned.id, owned.keep]);
        await page.evaluate(({ invalidation, id }) => {
          if (invalidation === 'disableAll') {
            const settings = JSON.parse(localStorage.getItem('4chan-settings'));
            localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, disableAll: true }));
            window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings', storageArea: localStorage }));
          } else {
            const form = document.querySelector(`#p${id} form[action="/fixture/delete"]`);
            form.replaceWith(form.cloneNode(true));
          }
        }, { invalidation, id: owned.reply });
        await expect(feedback(page)).toContainText(unknown);
        held.release();
        await held.close();
        await expect(page.locator(`#pc${owned.reply}`)).not.toHaveClass(/\bdeleted\b/);
        await expect(page.locator(`#p${owned.reply}`)).not.toHaveAttribute('aria-busy');
        if (invalidation === 'disableAll') {
          await page.evaluate(() => {
            const settings = JSON.parse(localStorage.getItem('4chan-settings'));
            localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, disableAll: false }));
            window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings', storageArea: localStorage }));
          });
        }
        const dialogs = [];
        page.on('dialog', async dialog => { dialogs.push(dialog.message()); await dialog.dismiss(); });
        await trigger(page, owned.reply).click();
        await page.getByRole('menuitem', { name: 'Delete post', exact: true }).click();
        await page.locator(`#p${owned.reply} form[action="/fixture/delete"]`).evaluate(form => form.requestSubmit());
        await expect(feedback(page)).toContainText(unknown);
        expect(dialogs).toEqual([]);
        expect(observed).toHaveLength(1);
        await expect(page).toHaveURL(`${origin}${owned.url}`);
        await page.reload();
        await expect(page.locator(`#pc${owned.reply}`)).toHaveCount(0);
        await expect(page.locator(`#pc${owned.keep}`)).toBeVisible();
      });
    } finally { await held.close(); }
  });
}

test('effective never-mobile layout, desktop and archived targets never offer the mobile deletion action', async ({ page, request, owned }) => {
  await page.addInitScript(() => localStorage.setItem('4chan_never_show_mobile', 'true'));
  await open(page, owned);
  const observed = writes(page);
  await trigger(page, owned.reply).click();
  await expect(page.getByRole('menuitem', { name: 'Delete post', exact: true })).toHaveCount(0);
  await expect(page.getByRole('menuitem', { name: 'Report post', exact: true })).toBeVisible();
  await expect(page.locator('#post-menu')).toHaveCSS('font-size', '12px');
  await page.evaluate(() => {
    localStorage.removeItem('4chan_never_show_mobile');
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile', storageArea: localStorage }));
  });
  await trigger(page, owned.reply).click();
  await expect(page.getByRole('menuitem', { name: 'Delete post', exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 1280, height: 900 });
  await trigger(page, owned.reply).click();
  await expect(page.getByRole('menuitem', { name: 'Delete post', exact: true })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 390, height: 844 });
  // A DOM-only archived state probes client gating; no server policy is changed.
  await page.locator(`#t${owned.id}`).evaluate(section => { section.dataset.archived = 'true'; });
  await trigger(page, owned.reply).click();
  await expect(page.getByRole('menuitem', { name: 'Delete post', exact: true })).toHaveCount(0);
  expect(await ids(request, owned)).toEqual([owned.id, owned.reply, owned.keep]);
  expect(observed).toHaveLength(0);
});
