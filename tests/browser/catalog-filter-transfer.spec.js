import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const lockName = 'paperboard-thread-watcher';
const keys = ['4chan-settings', '4chan-filters', '4chan-css', 'catalog-filters', 'catalog-settings'];
const cfg = payload => '#cfg=' + encodeURIComponent(JSON.stringify(payload));
const rule = { active: 1, pattern: '/.*/', boards: '', hidden: 1, top: 0 };
const rawRules = JSON.stringify({ 7: rule });
const incomingSettings = JSON.stringify({ disableAll: true, filter: false, quotePreview: false });
const payload = { settings: incomingSettings, catalogFilters: rawRules };
const initial = { '4chan-settings': '{"disableAll":true}', 'catalog-filters': '{}' };
const read = page => page.evaluate(keys => Object.fromEntries(keys.map(key => [key, localStorage.getItem(key)])), keys);
const ids = page => page.locator('#threads > .thread').evaluateAll(nodes => nodes.map(node => node.dataset.threadId));
const review = page => page.getByRole('dialog', { name: 'Restore Settings', exact: true });

async function prepare(page, values = initial, path = '/demo/') {
  await page.addInitScript(({ keys, values }) => {
    for (const key of keys) localStorage.removeItem(key);
    for (const [key, raw] of Object.entries(values)) localStorage.setItem(key, raw);
  }, { keys, values });
  await page.goto(path);
}
async function openRestore(page, incoming = payload) {
  await page.evaluate(hash => { location.hash = hash; }, cfg(incoming));
  await expect(review(page)).toBeVisible();
}
async function holdLock(other) {
  await other.goto('/');
  await other.evaluate(name => new Promise(resolve => {
    window.heldTransferLock = navigator.locks.request(name, async () => {
      resolve(); await new Promise(release => { window.releaseTransferLock = release; });
    });
  }), lockName);
}
async function releaseLock(other) {
  await other.evaluate(async () => { window.releaseTransferLock(); await window.heldTransferLock; });
}

test('catalog filter restore refreshes the current catalog once, keeps pins and supports clearing rules', async ({ page, browser }) => {
  const errors = [], workers = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('worker', worker => workers.push(worker.url()));
  const writerContext = await browser.newContext({ javaScriptEnabled: false });
  const writer = await writerContext.newPage(), created = [];
  const password = 'owned-catalog-transfer-fixture';
  try {
    for (let index = 0; index < 2; index++) {
      await writer.goto('/fixture/');
      await writer.locator('#sub').fill(`Owned catalog transfer ${index}`);
      await writer.locator('#com').fill('Paper fixture for preference restoration.');
      await expect(writer.locator('#postPassword')).toHaveValue('');
      const response = writer.waitForResponse(response => response.url().endsWith('/fixture/imgboard.php') && response.request().method() === 'POST');
      await writer.getByRole('button', { name: 'Post', exact: true }).click();
      expect((await response).status()).toBe(303);
      await expect(writer).toHaveURL(/\/thread\/\d+#p\d+$/);
      created.push(/#p(\d+)$/.exec(writer.url())[1]);
    }
    await prepare(page, initial, '/fixture/catalog');
    const originalIds = await ids(page); expect(originalIds.length).toBeGreaterThan(1);
    const pinned = originalIds[0];
    await page.locator(`#thread-${pinned}`).hover();
    await page.getByRole('button', { name: `Thread ${pinned} menu`, exact: true }).click();
    await page.getByRole('menuitem', { name: 'Pin thread', exact: true }).click();
    const pinBefore = await page.evaluate(() => localStorage.getItem('4chan-pin-fixture'));
    const theme = '{"nobinds":true,"css":".teaser { color: #112233; }"}';
    await page.evaluate(theme => localStorage.setItem('catalog-theme', theme), theme);
    let navigations = 0; page.on('request', request => { if (request.isNavigationRequest()) navigations++; });
    const display = JSON.stringify({ orderby: 'r', large: true, extended: false });
    await openRestore(page, { ...payload, catalogSettings: display });
    expect(await read(page)).toMatchObject(initial);
    await expect(review(page).locator('.settingsTransferCatalogFilters pre')).toHaveText(rawRules);
    await page.evaluate(() => {
      window.catalogRestoreRenders = 0;
      // Moving existing cards into the fragment also emits removal records.
      // Count the completed content insertions, not those preparatory removals.
      new MutationObserver(records => { catalogRestoreRenders += records.filter(record => record.addedNodes.length).length; })
        .observe(document.getElementById('threads'), { childList: true });
    });
    await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
    await expect(review(page).getByRole('status')).toHaveText('Settings restored.');
    await expect.poll(() => ids(page)).toEqual([pinned]);
    await expect(page.locator('#size-ctrl')).toHaveValue('large');
    await expect(page.locator('#teaser-ctrl')).toHaveValue('off');
    expect(await page.evaluate(() => localStorage.getItem('4chan-pin-fixture'))).toBe(pinBefore);
    expect(await page.evaluate(() => localStorage.getItem('catalog-theme'))).toBe(theme);
    expect(workers).toContain(origin + '/static/catalog-filter-core.v1.js');
    expect(await read(page)).toMatchObject({ '4chan-settings': incomingSettings, 'catalog-filters': rawRules, 'catalog-settings': display });
    expect(await page.evaluate(() => catalogRestoreRenders)).toBe(1);
    await review(page).getByRole('button', { name: 'Close', exact: true }).click();
    await openRestore(page, { settings: incomingSettings, catalogFilters: '{}' });
    await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
    await expect(review(page).getByRole('status')).toHaveText('Settings restored.');
    await expect.poll(() => ids(page)).toHaveLength(originalIds.length);
    expect((await ids(page))[0]).toBe(pinned);
    expect((await read(page))['catalog-filters']).toBe('{}');
    expect(await page.evaluate(() => catalogRestoreRenders)).toBe(2);
    expect(navigations).toBe(0); expect(errors).toEqual([]);
  } finally {
    for (const id of created) {
      await writer.goto(`/fixture/thread/${id}`);
      const actions = writer.locator(`#p${id} .postActions`);
      await actions.getByText('Delete or report', { exact: true }).click();
      await expect(actions.locator('input[name=password]')).toHaveValue('');
      await actions.getByRole('button', { name: 'Delete post', exact: true }).click();
    }
    await writerContext.close();
  }
});

test('catalog syntax validation includes other-board rules without changing their reviewed scope', async ({ page }) => {
  await prepare(page);
  const bad = JSON.stringify({ 0: { ...rule, pattern: '/[/', boards: 'other' } });
  await openRestore(page, { settings: incomingSettings, catalogFilters: bad });
  const worker = page.waitForEvent('worker');
  await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
  expect((await worker).url()).toBe(origin + '/static/catalog-filter-core.v1.js');
  await expect(review(page).getByRole('status')).toContainText('Nothing was restored');
  expect(await read(page)).toMatchObject(initial);
  await review(page).getByRole('button', { name: 'Cancel', exact: true }).click();
  const valid = JSON.stringify({ 7: { ...rule, boards: 'other' } });
  await openRestore(page, { settings: incomingSettings, catalogFilters: valid });
  await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await expect(review(page).getByRole('status')).toHaveText('Settings restored.');
  expect((await read(page))['catalog-filters']).toBe(valid);
});

test('an already-open catalog editor must reopen after settings transfer before it can save', async ({ page }) => {
  await prepare(page, initial, '/demo/catalog');
  await page.locator('#filters-ctrl').click(); await page.locator('#filters-add').click();
  await page.locator('#filters .filter-pattern').fill('stale editor');
  await openRestore(page);
  await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await expect(review(page).getByRole('status')).toHaveText('Settings restored.');
  await review(page).getByRole('button', { name: 'Close', exact: true }).click();
  await expect(page.locator('#filters-msg')).toContainText('Reopen the editor');
  await page.locator('#filters-save').click();
  await expect(page.locator('#filters-msg')).toContainText('Reopen the editor');
  expect((await read(page))['catalog-filters']).toBe(rawRules);
  await page.locator('#filters-close').click(); await page.locator('#filters-ctrl').click();
  await expect(page.locator('#filters .filter-pattern')).toHaveValue(rule.pattern);
});

test('valid catalog rules apply even when unrelated stored display preferences are malformed', async ({ page }) => {
  await prepare(page, { ...initial, 'catalog-settings': 'not JSON' }, '/demo/catalog');
  expect((await ids(page)).length).toBeGreaterThan(0);
  await openRestore(page);
  await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await expect(review(page).getByRole('status')).toHaveText('Settings restored.');
  await expect.poll(() => ids(page)).toEqual([]);
  await expect(page.locator('#catalog-preference-status')).toContainText('Restored filters use the current display');
  expect(await read(page)).toMatchObject({ '4chan-settings': incomingSettings, 'catalog-filters': rawRules, 'catalog-settings': 'not JSON' });
  expect(new URL(page.url()).search).toBe('');
});

test('native-only restore preserves tab-only catalog rules and explicit rule restore does not grant persistence', async ({ page }) => {
  await prepare(page, initial, '/demo/catalog');
  const count = (await ids(page)).length; expect(count).toBeGreaterThan(0);
  await page.evaluate(() => {
    const set = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (key === 'catalog-filters') throw new DOMException('Owned catalog write denial', 'QuotaExceededError');
      return set.call(this, key, value);
    };
    window.allowCatalogTransferWrite = () => { Storage.prototype.setItem = set; };
  });
  await page.locator('#filters-ctrl').click(); await page.locator('#filters-add').click();
  await page.locator('#filters .filter-pattern').fill(rule.pattern);
  await page.locator('#filters .filter-hide').check(); await page.locator('#filters-save').click();
  await expect(page.locator('#filters-msg')).toContainText('only in this tab');
  await expect.poll(() => ids(page)).toEqual([]);
  await page.locator('#filters-close').click();
  await page.evaluate(() => allowCatalogTransferWrite());
  await openRestore(page, { settings: incomingSettings });
  await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await expect(review(page).getByRole('status')).toHaveText('Settings restored.');
  await page.evaluate(() => new Promise(requestAnimationFrame));
  expect(await ids(page)).toEqual([]); expect((await read(page))['catalog-filters']).toBe('{}');
  await review(page).getByRole('button', { name: 'Close', exact: true }).click();
  await openRestore(page, { settings: incomingSettings, catalogFilters: '{}' });
  await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await expect(review(page).getByRole('status')).toHaveText('Settings restored.');
  await expect.poll(() => ids(page)).toHaveLength(count);
  await review(page).getByRole('button', { name: 'Close', exact: true }).click();
  await page.locator('#filters-ctrl').click(); await page.locator('#filters-add').click();
  await page.locator('#filters .filter-pattern').fill(rule.pattern);
  await page.locator('#filters .filter-hide').check(); await page.locator('#filters-save').click();
  await expect(page.locator('#filters-msg')).toContainText('only in this tab');
  expect((await read(page))['catalog-filters']).toBe('{}');
});

for (const interruption of ['cancel', 'detach', 'BFCache', 'newer-storage']) {
  test(`catalog validation ${interruption} leaves reviewed keys unchanged`, async ({ page, context }) => {
    await prepare(page);
    const other = interruption === 'newer-storage' ? await context.newPage() : null;
    if (other) await other.goto('/');
    await openRestore(page);
    await page.evaluate(() => {
      const post = Worker.prototype.postMessage, stop = Worker.prototype.terminate;
      window.heldCatalogValidation = false; window.catalogStops = 0;
      Worker.prototype.terminate = function () { catalogStops++; return stop.call(this); };
      Worker.prototype.postMessage = function (...args) {
        const job = typeof args[0] === 'string' ? JSON.parse(args[0]) : null;
        if (job?.version === 1 && job.cards.length === 0 && job.rules.length === 1) {
          window.heldCatalogValidation = true;
          window.releaseCatalogValidation = () => post.apply(this, args);
          Worker.prototype.postMessage = post; return;
        }
        return post.apply(this, args);
      };
    });
    await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
    await expect.poll(() => page.evaluate(() => heldCatalogValidation)).toBe(true);
    const newer = JSON.stringify({ 0: { ...rule, pattern: 'newer' } });
    if (interruption === 'cancel') await review(page).getByRole('button', { name: 'Cancel', exact: true }).click();
    else if (interruption === 'detach') await page.evaluate(() => document.body.remove());
    else if (interruption === 'BFCache') await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
    else await other.evaluate(raw => localStorage.setItem('catalog-filters', raw), newer);
    await page.evaluate(() => releaseCatalogValidation());
    if (interruption === 'newer-storage') {
      await expect(review(page).getByRole('status')).toContainText('changed after this review opened');
      await other.close();
    } else await expect(review(page)).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => catalogStops)).toBe(1);
    expect(await read(page)).toMatchObject({ ...initial, 'catalog-filters': interruption === 'newer-storage' ? newer : '{}' });
    if (interruption === 'BFCache') {
      await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
      await expect(review(page)).toBeVisible();
      expect(await read(page)).toMatchObject(initial);
    }
  });
}

test('catalog restore waits for the editor shared lock and preserves a competing catalog write', async ({ page, context }) => {
  await prepare(page); await openRestore(page);
  const other = await context.newPage(); await holdLock(other);
  try {
    await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
    await expect.poll(() => page.evaluate(async name => (await navigator.locks.query()).pending.filter(lock => lock.name === name).length, lockName)).toBe(1);
    expect(await read(page)).toMatchObject(initial);
    const newer = JSON.stringify({ 0: { ...rule, pattern: 'newer' } });
    await other.evaluate(raw => localStorage.setItem('catalog-filters', raw), newer);
    await releaseLock(other);
    await expect(review(page).getByRole('status')).toContainText('changed after this review opened');
    expect(await read(page)).toMatchObject({ ...initial, 'catalog-filters': newer });
  } finally { await other.close(); }
});

for (const partial of [false, true]) {
  test(`five-key catalog restore reports ${partial ? 'partial' : 'complete'} rollback and keeps unrelated state`, async ({ page }) => {
    const old = { ...initial, '4chan-filters': '[]', '4chan-css': '.reply { color: #112233; }',
      'catalog-settings': '{"orderby":"alt","large":false,"extended":true}' };
    const next = { ...payload, filters: '[]', css: '.reply { color: #334455; }',
      catalogSettings: '{"orderby":"r","large":true,"extended":false}' };
    await prepare(page, old); await openRestore(page, next);
    const replacement = JSON.stringify({ 0: { ...rule, pattern: 'nonparticipating writer' } });
    await page.evaluate(({ partial, replacement }) => {
      const original = Storage.prototype.setItem;
      window.transferEvents = 0; window.transferWrites = [];
      document.addEventListener('4chanPreferencesRestored', () => transferEvents++);
      localStorage.setItem('owned-private-receipt', 'keep');
      Storage.prototype.setItem = function (key, value) {
        transferWrites.push(key);
        if (key === '4chan-settings') {
          if (partial) original.call(this, 'catalog-filters', replacement);
          throw new DOMException('Owned quota failure', 'QuotaExceededError');
        }
        return original.call(this, key, value);
      };
    }, { partial, replacement });
    await review(page).getByRole('button', { name: 'Restore Settings', exact: true }).click();
    await expect(review(page).getByRole('status')).toContainText(partial ? 'rollback could not fully restore' : 'previous stored values were restored');
    expect(await read(page)).toEqual({ ...old, 'catalog-filters': partial ? replacement : '{}' });
    const evidence = await page.evaluate(() => ({ writes: transferWrites, events: transferEvents, receipt: localStorage.getItem('owned-private-receipt') }));
    expect(evidence.writes.slice(0, 5)).toEqual(['4chan-filters', '4chan-css', 'catalog-filters', 'catalog-settings', '4chan-settings']);
    expect(evidence.events).toBe(0); expect(evidence.receipt).toBe('keep');
  });
}
