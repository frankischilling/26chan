import { watcherSettingsOpener } from './helpers/watcher-settings.js';
import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const settingsKey = '4chan-settings';
const filtersKey = '4chan-filters';
const cssKey = '4chan-css';
const catalogKey = 'catalog-settings';

const settings = JSON.stringify({
  quotePreview: true,
  customCSS: true,
  threadWatcher: false,
  customMenu: true,
  customMenuList: 'demo test',
});
const filters = JSON.stringify([{
  type: 2,
  pattern: '<img src=/settings-transfer-attack onerror=alert(1)>',
  boards: 'demo',
  active: true,
  auto: false,
  hide: false,
}]);
const css = '.reply { color: #334455; padding-left: 8px; }';
const catalog = JSON.stringify({ orderby: 'r', large: true, extended: false });
const catalogFilters = JSON.stringify({ 7: { active: 1, pattern: '<img src=/settings-transfer-attack>', boards: '', hidden: 0, top: 0 } });
const cfg = payload => `#cfg=${encodeURIComponent(JSON.stringify(payload))}`;

async function openSettings(page) {
  await watcherSettingsOpener(page).first().click();
  const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
  await expect(dialog).toBeVisible();
  return dialog;
}

test('real settings export is canonical, inert and excludes unrelated browser state', async ({ page }) => {
  const attackRequests = [];
  page.on('request', request => {
    if (request.url().includes('settings-transfer-attack')) attackRequests.push(request.url());
  });
  await page.addInitScript(({ settings, filters, css, catalog, catalogFilters }) => {
    localStorage.setItem('4chan-settings', settings);
    localStorage.setItem('4chan-filters', filters);
    localStorage.setItem('4chan-css', css);
    localStorage.setItem('catalog-settings', catalog);
    localStorage.setItem('catalog-filters', catalogFilters);
    localStorage.setItem('catalog-theme', '{"newtab":true}');
    localStorage.setItem('4chan-watch', '{"private-watch":"do-not-export"}');
    localStorage.setItem('4chan-watch-bl', '{"private-blacklist":1}');
    localStorage.setItem('owned-password-fixture', 'do-not-export-password');
  }, { settings, filters, css, catalog, catalogFilters });
  const response = await page.goto('/demo/0?ignored=1');
  expect(response.status()).toBe(200);

  const settingsDialog = await openSettings(page);
  await settingsDialog.getByRole('button', { name: 'Export Settings', exact: true }).click();
  const exportDialog = page.getByRole('dialog', { name: 'Export Settings', exact: true });
  await expect(exportDialog).toBeVisible();
  const field = exportDialog.getByLabel('Settings export URL', { exact: true });
  await expect(field).toHaveAttribute('readonly', '');
  const url = await field.inputValue();
  expect(url.startsWith(`${origin}/demo/#cfg=`)).toBe(true);
  expect(url).not.toContain('/0?ignored=1');
  await expect(exportDialog.getByRole('link', { name: 'Restore Settings', exact: true })).toHaveAttribute('href', url);
  await expect(exportDialog.locator('img, script, style')).toHaveCount(0);

  const payload = JSON.parse(decodeURIComponent(new URL(url).hash.slice(5)));
  expect(payload).toEqual({ settings, filters, css, catalogFilters, catalogSettings: catalog });
  expect(payload).not.toHaveProperty('catalogTheme');
  expect(JSON.stringify(payload)).not.toContain('private-watch');
  expect(JSON.stringify(payload)).not.toContain('private-blacklist');
  expect(JSON.stringify(payload)).not.toContain('do-not-export-password');
  expect(attackRequests).toEqual([]);
});

test('restore link reviews every included preference before the shared transaction writes anything', async ({ page }) => {
  const incomingSettings = JSON.stringify({ quotePreview: false, customCSS: true, disableAll: true });
  const incomingFilters = JSON.stringify([{
    type: 5, pattern: 'paper', boards: 'demo', active: true, auto: false, hide: false,
  }]);
  const incomingCSS = '.postMessage { color: #556677; font-size: 14px; }';
  const incomingCatalog = JSON.stringify({ orderby: 'absdate', large: false, extended: true });
  const currentSettings = JSON.stringify({ quotePreview: true, customCSS: false, disableAll: false });
  await page.addInitScript(current => {
    localStorage.setItem('4chan-settings', current);
    localStorage.removeItem('4chan-filters');
    localStorage.removeItem('4chan-css');
    localStorage.removeItem('catalog-settings');
  }, currentSettings);

  await page.goto(`/demo/${cfg({
    settings: incomingSettings,
    filters: incomingFilters,
    css: incomingCSS,
    catalogSettings: incomingCatalog,
  })}`);
  const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await expect(review).toBeVisible();
  await expect(page).toHaveURL(`${origin}/demo/`);
  await expect(review).toContainText('Nothing is changed until you choose Restore Settings.');
  await expect(review).toContainText('Catalog display preferences');

  expect(await page.evaluate(() => ({
    settings: localStorage.getItem('4chan-settings'),
    filters: localStorage.getItem('4chan-filters'),
    css: localStorage.getItem('4chan-css'),
    catalog: localStorage.getItem('catalog-settings'),
  }))).toEqual({ settings: currentSettings, filters: null, css: null, catalog: null });

  await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await expect(review.getByRole('status')).toHaveText('Settings restored.');
  await expect(review.getByRole('button', { name: 'Restore Settings', exact: true })).toBeDisabled();
  await expect(page).toHaveURL(`${origin}/demo/`);
  expect(await page.evaluate(() => ({
    settings: localStorage.getItem('4chan-settings'),
    filters: localStorage.getItem('4chan-filters'),
    css: localStorage.getItem('4chan-css'),
    catalog: localStorage.getItem('catalog-settings'),
  }))).toEqual({ settings: incomingSettings, filters: incomingFilters, css: incomingCSS, catalog: incomingCatalog });
});

test('stale cross-tab values block restore after review', async ({ page, context }) => {
  const current = JSON.stringify({ quotePreview: true, backlinks: true });
  const incoming = JSON.stringify({ quotePreview: false, backlinks: true });
  const newer = JSON.stringify({ quotePreview: true, backlinks: false });
  await page.addInitScript(current => localStorage.setItem('4chan-settings', current), current);
  await page.goto(`/demo/${cfg({ settings: incoming })}`);
  const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await expect(review).toBeVisible();

  const other = await context.newPage();
  try {
    await other.goto('/demo/');
    await other.evaluate(newer => localStorage.setItem('4chan-settings', newer), newer);
    await expect.poll(() => page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(newer);
    await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
    await expect(review.getByRole('status')).toContainText('changed after this review opened');
    expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(newer);
  } finally { await other.close(); }
});

test('valid restore can repair malformed existing values above the normal preference limits', async ({ page }) => {
  await page.addInitScript(() => {
    for (const [key, size] of [['4chan-settings', 4097], ['4chan-filters', 131073],
      ['4chan-css', 16385], ['catalog-settings', 1025]]) localStorage.setItem(key, 'x'.repeat(size));
  });
  const settings = JSON.stringify({ filter: false, customCSS: false, threadStats: false });
  const filters = '[]';
  const css = '.postMessage { color: #223344; }';
  const catalogSettings = JSON.stringify({ orderby: 'r', large: false, extended: false });
  await page.goto(`/demo/${cfg({ settings, filters, css, catalogSettings })}`);
  const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await expect(review).toBeVisible();
  await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await expect(review.getByRole('status')).toHaveText('Settings restored.');
  expect(await page.evaluate(() => Object.fromEntries(['4chan-settings', '4chan-filters', '4chan-css', 'catalog-settings']
    .map(key => [key, localStorage.getItem(key)])))).toEqual({
    '4chan-settings': settings, '4chan-filters': filters, '4chan-css': css, 'catalog-settings': catalogSettings,
  });
});

test('invalid restored filter colors and patterns cannot replace working preferences', async ({ page }) => {
  const current = JSON.stringify({ filter: false, threadStats: false });
  const incoming = JSON.stringify({ filter: false, threadStats: false, quotePreview: false });
  const rule = { type: 2, pattern: '/[/', boards: 'demo', active: true, hide: false };
  const attackRequests = [];
  page.on('request', request => { if (request.url().includes('restore-filter-attack')) attackRequests.push(request.url()); });
  await page.addInitScript(current => {
    localStorage.setItem('4chan-settings', current);
    localStorage.removeItem('4chan-filters');
  }, current);
  await page.goto(`/demo/${cfg({ settings: incoming, filters: JSON.stringify([
    { ...rule, pattern: 'needle', color: 'red; background:url(/restore-filter-attack)' },
  ]) })}`);
  await expect(page.locator('#settingsTransferError')).toContainText('filter color is invalid');
  expect(await page.evaluate(() => localStorage.getItem('4chan-filters'))).toBeNull();
  expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(current);

  await page.goto(`/demo/${cfg({ settings: incoming, filters: JSON.stringify([rule]) })}`);
  const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await expect(review).toBeVisible();
  const worker = page.waitForEvent('worker');
  await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  expect((await worker).url()).toBe(`${origin}/static/native-filter.v1.js`);
  await expect(review.getByRole('status')).toContainText('Nothing was restored');
  expect(await page.evaluate(() => localStorage.getItem('4chan-filters'))).toBeNull();
  expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(current);
  expect(attackRequests).toEqual([]);
});

test('filter restore checks syntax in a worker and source Quick Reply coordinates open safely', async ({ page }) => {
  const incomingSettings = JSON.stringify({ filter: false, threadStats: false, 'QR-position': 'right: 20px; top: 10%;' });
  const incomingFilters = JSON.stringify([
    { type: 2, pattern: '/(a+)+$/', boards: 'demo', active: true, color: '#334455' },
  ]);
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.goto(`/demo/${cfg({ settings: incomingSettings, filters: incomingFilters })}`);
  const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await expect(review).toBeVisible();
  const worker = page.waitForEvent('worker');
  await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  expect((await worker).url()).toBe(`${origin}/static/native-filter.v1.js`);
  await expect(review.getByRole('status')).toHaveText('Settings restored.');
  expect(await page.evaluate(() => localStorage.getItem('4chan-filters'))).toBe(incomingFilters);
  await review.getByRole('button', { name: 'Close', exact: true }).click();
  await page.goto('/demo/thread/1000001');
  await page.locator('.open-qr-link').click();
  const qr = page.locator('#quickReply');
  await expect(qr).toBeVisible();
  const box = await qr.boundingBox();
  expect(box.x + box.width).toBeCloseTo(980, 0);
  expect(box.y).toBeCloseTo(80, 0);
  await page.locator('#qrCom').fill('Unsubmitted position check');
  // The browser schedules resize delivery after setViewportSize returns.
  await page.evaluate(() => {
    window.ownedQuickReplyResize = new Promise(resolve => window.addEventListener('resize', resolve, { once: true }));
  });
  await page.setViewportSize({ width: 640, height: 700 });
  await page.evaluate(() => window.ownedQuickReplyResize);
  const resized = await qr.boundingBox();
  expect(resized.x).toBeGreaterThanOrEqual(0);
  expect(resized.x + resized.width).toBeLessThanOrEqual(640);
  await expect(page.locator('#qrCom')).toHaveValue('Unsubmitted position check');
});

for (const interruption of ['cancel', 'newer-storage']) {
  test(`filter validation ${interruption} cannot commit a stale restore`, async ({ page }) => {
    const current = JSON.stringify({ filter: false, threadStats: false });
    const newer = JSON.stringify({ filter: false, threadStats: false, quotePreview: true });
    const incoming = JSON.stringify({ filter: false, threadStats: false, quotePreview: false });
    await page.addInitScript(current => {
      localStorage.setItem('4chan-settings', current);
      localStorage.removeItem('4chan-filters');
    }, current);
    await page.goto(`/demo/${cfg({ settings: incoming, filters: JSON.stringify([
      { type: 2, pattern: '/needle/i', boards: 'demo', active: true },
    ]) })}`);
    const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
    await expect(review).toBeVisible();
    await page.evaluate(() => {
      const postMessage = Worker.prototype.postMessage;
      window.heldFilterValidation = false;
      Worker.prototype.postMessage = function (...args) {
        const job = typeof args[0] === 'string' ? JSON.parse(args[0]) : null;
        if (job?.mode === 'page' && job.posts.length === 0 && job.filters.length === 1) {
          window.heldFilterValidation = true;
          window.releaseFilterValidation = () => postMessage.apply(this, args);
          Worker.prototype.postMessage = postMessage;
          return;
        }
        return postMessage.apply(this, args);
      };
    });
    await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
    await expect.poll(() => page.evaluate(() => window.heldFilterValidation)).toBe(true);
    if (interruption === 'cancel') await review.getByRole('button', { name: 'Cancel', exact: true }).click();
    else await page.evaluate(newer => localStorage.setItem('4chan-settings', newer), newer);
    await page.evaluate(() => window.releaseFilterValidation());
    if (interruption === 'newer-storage') await expect(review.getByRole('status')).toContainText('changed after this review opened');
    else await expect(review).toHaveCount(0);
    expect(await page.evaluate(() => localStorage.getItem('4chan-filters'))).toBeNull();
    expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(interruption === 'cancel' ? current : newer);
  });
}

test('malformed catalog filters and dangerous CSS stop before storage writes or network activity', async ({ page }) => {
  const attackRequests = [];
  page.on('request', request => {
    if (request.url().includes('settings-transfer-attack')) attackRequests.push(request.url());
  });
  const current = JSON.stringify({ quotePreview: true });
  await page.addInitScript(current => localStorage.setItem('4chan-settings', current), current);

  await page.goto(`/demo/${cfg({ settings: JSON.stringify({ quotePreview: false }), catalogFilters: '[]' })}`);
  let error = page.locator('#settingsTransferError');
  await expect(error).toBeVisible();
  await expect(error).toContainText('supported catalog rule format');
  expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(current);
  await error.getByRole('button', { name: 'Close', exact: true }).click();

  await page.goto(`/demo/${cfg({
    settings: JSON.stringify({ quotePreview: false }),
    css: '.reply { background-image: url(/settings-transfer-attack.png); display: none; }',
  })}`);
  error = page.locator('#settingsTransferError');
  await expect(error).toBeVisible();
  await expect(error).toContainText('Custom CSS is invalid');
  expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(current);
  expect(attackRequests).toEqual([]);
});

test('quota failure rolls back values already written before reporting the restore failure', async ({ page }) => {
  const currentSettings = JSON.stringify({ quotePreview: true, backlinks: true });
  const currentFilters = JSON.stringify([{
    type: 5, pattern: 'old', boards: 'demo', active: true, auto: false, hide: false,
  }]);
  const incomingSettings = JSON.stringify({ quotePreview: false, backlinks: false });
  const incomingFilters = JSON.stringify([{
    type: 5, pattern: 'new', boards: 'demo', active: true, auto: false, hide: false,
  }]);
  await page.addInitScript(({ currentSettings, currentFilters }) => {
    localStorage.setItem('4chan-settings', currentSettings);
    localStorage.setItem('4chan-filters', currentFilters);
  }, { currentSettings, currentFilters });
  await page.goto(`/demo/${cfg({ settings: incomingSettings, filters: incomingFilters })}`);
  const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await expect(review).toBeVisible();
  await page.evaluate(() => {
    const original = Storage.prototype.setItem;
    let failed = false;
    window.restoreTransferSetItem = original;
    window.transferWriteAttempts = [];
    Storage.prototype.setItem = function (key, value) {
      window.transferWriteAttempts.push({ key, value });
      if (key === '4chan-settings' && !failed) {
        failed = true;
        throw new DOMException('Owned quota failure', 'QuotaExceededError');
      }
      return original.call(this, key, value);
    };
  });
  try {
    await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
    await expect(review.getByRole('status')).toContainText('previous stored values were restored');
  } finally {
    await page.evaluate(() => { Storage.prototype.setItem = window.restoreTransferSetItem; });
  }
  expect(await page.evaluate(() => window.transferWriteAttempts)).toEqual([
    { key: filtersKey, value: incomingFilters },
    { key: settingsKey, value: incomingSettings },
    { key: filtersKey, value: currentFilters },
  ]);
  expect(await page.evaluate(() => ({
    settings: localStorage.getItem('4chan-settings'), filters: localStorage.getItem('4chan-filters'),
  }))).toEqual({ settings: currentSettings, filters: currentFilters });
});

test('restore stays unavailable when the shared lock cannot be acquired', async ({ page }) => {
  const current = JSON.stringify({ quotePreview: true });
  const incoming = JSON.stringify({ quotePreview: false });
  await page.addInitScript(current => {
    localStorage.setItem('4chan-settings', current);
    const locks = navigator.locks;
    if (locks) Object.defineProperty(locks, 'request', {
      configurable: true,
      value: async () => { throw new DOMException('Owned lock denial', 'SecurityError'); },
    });
  }, current);
  await page.goto(`/demo/${cfg({ settings: incoming })}`);
  const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await expect(review).toBeVisible();
  await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
  await expect(review.getByRole('status')).toContainText('persistent browser storage or cross-tab locking is unavailable');
  expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(current);
});

test('restore stays unavailable if browser storage becomes unreadable after review', async ({ page }) => {
  const current = JSON.stringify({ quotePreview: true });
  const incoming = JSON.stringify({ quotePreview: false });
  await page.addInitScript(current => localStorage.setItem('4chan-settings', current), current);
  await page.goto(`/demo/${cfg({ settings: incoming })}`);
  const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
  await expect(review).toBeVisible();
  await page.evaluate(() => {
    window.restoreTransferGetItem = Storage.prototype.getItem;
    Storage.prototype.getItem = function () { throw new DOMException('Owned storage denial', 'SecurityError'); };
  });
  try {
    await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
    await expect(review.getByRole('status')).toContainText('persistent browser storage or cross-tab locking is unavailable');
  } finally {
    await page.evaluate(() => { Storage.prototype.getItem = window.restoreTransferGetItem; });
  }
  expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(current);
});

for (const confirmBeforeRelease of [false, true]) {
  test(`first-run initialization cannot invalidate a sparse restore ${confirmBeforeRelease ? 'queued behind its lock' : 'awaiting review'}`, async ({ page, context }) => {
    await page.goto('/fixture/');
    expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBeNull();
    const other = await context.newPage();
    await other.goto('/');
    await other.evaluate(() => new Promise(resolve => {
      window.restoreRaceLockDone = navigator.locks.request('paperboard-thread-watcher', async () => {
        resolve();
        await new Promise(release => { window.releaseRestoreRaceLock = release; });
      });
    }));
    try {
      await openSettings(page);
      await expect.poll(() => page.evaluate(async () => (await navigator.locks.query()).pending
        .filter(lock => lock.name === 'paperboard-thread-watcher').length)).toBe(1);
      const incoming = JSON.stringify({ quotePreview: false });
      await page.evaluate(hash => { location.hash = hash; }, cfg({ settings: incoming }));
      const review = page.getByRole('dialog', { name: 'Restore Settings', exact: true });
      await expect(review).toBeVisible();
      await expect(page).toHaveURL(`${origin}/fixture/`);
      expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBeNull();
      if (confirmBeforeRelease) {
        await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
        await expect(review.getByRole('status')).toHaveText('Restoring settings...');
      }
      await other.evaluate(async () => { window.releaseRestoreRaceLock(); await window.restoreRaceLockDone; });
      // A barrier behind initialization proves the pending review cannot cause a
      // default write, rather than checking storage before a queued job runs.
      await page.evaluate(() => navigator.locks.request('paperboard-thread-watcher', () => {}));
      if (!confirmBeforeRelease) {
        expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBeNull();
        await review.getByRole('button', { name: 'Restore Settings', exact: true }).click();
      }
      await expect(review.getByRole('status')).toHaveText('Settings restored.');
      await expect(page.locator('#settingsMenu')).toHaveCount(0);
      expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(incoming);
      await review.getByRole('button', { name: 'Close', exact: true }).click();
      const reopened = await openSettings(page);
      await expect(reopened.getByRole('button', { name: 'Export Settings', exact: true })).toBeEnabled();
      await page.evaluate(() => navigator.locks.request('paperboard-thread-watcher', () => {}));
      expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(incoming);
      await reopened.getByRole('button', { name: 'Export Settings', exact: true }).click();
      const exported = await page.getByRole('dialog', { name: 'Export Settings', exact: true })
        .getByLabel('Settings export URL', { exact: true }).inputValue();
      expect(JSON.parse(decodeURIComponent(new URL(exported).hash.slice(5))).settings).toBe(incoming);
    } finally { await other.close(); }
  });
}
