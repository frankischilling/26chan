import { test, expect } from '@playwright/test';
import { openSettingControl, openWatcherSettings, saveWatcherSettings, watcherSettingsOpener } from './helpers/watcher-settings.js';

test('catalog settings discard cancelled edits and save the native watcher flag without navigation', async ({ page }) => {
  await page.goto('/fixture/catalog');
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest()) navigations++; });
  let dialog = await openWatcherSettings(page);
  await expect(dialog).toHaveAttribute('id', 'theme');
  await expect(dialog.locator('#theme-nobinds')).toBeFocused();
  await dialog.getByLabel('Thread Watcher', { exact: true }).check();
  await page.keyboard.press('Escape');
  await expect(page.locator('#settingsWindowLink')).toBeFocused();
  await expect(page.locator('#threadWatcher')).toBeHidden();
  expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBeNull();
  await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true, threadWatcher: true, threadAutoWatcher: true, unrelated: 'keep' })));
  dialog = await openWatcherSettings(page);
  await expect(dialog.getByLabel('Thread Watcher', { exact: true })).not.toBeChecked();
  await dialog.getByRole('button', { name: 'Close settings' }).click();
  await saveWatcherSettings(page, { threadWatcher: true });
  await expect(page.locator('#threadWatcher')).toBeVisible();
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toEqual({ disableAll: false, threadWatcher: true, threadAutoWatcher: true, unrelated: 'keep', dropDownNav: false });
  expect(navigations).toBe(0);
});

test('Monitoring saves auto-watch and fixed placement; Disable overrides checked options', async ({ page }) => {
  await page.goto('/fixture/');
  await saveWatcherSettings(page, { threadWatcher: true, threadAutoWatcher: true, fixedThreadWatcher: true });
  const panel = page.locator('#threadWatcher');
  await expect(panel).toHaveCSS('position', 'fixed');
  await expect(panel).toHaveCSS('top', '380px');
  await expect(page.locator('input[name=awt]')).toHaveValue('1');
  await expect(page.locator('input[name=track]')).toHaveValue('1');
  await page.evaluate(() => { const spacer = document.createElement('div'); spacer.style.height = '2000px'; document.body.append(spacer); scrollTo(0, 200); });
  expect((await panel.boundingBox()).y).toBe(380);
  await page.evaluate(() => scrollTo(0, 0));
  await saveWatcherSettings(page, { disableAll: true });
  await expect(panel).toBeHidden();
  await expect(page.locator('input[name=awt], input[name=track]')).toHaveCount(0);
  const dialog = await openWatcherSettings(page);
  await expect(dialog.getByLabel('Thread Watcher', { exact: true })).toBeChecked();
  await expect(dialog.getByLabel('Disable the native extension', { exact: true })).toBeChecked();
});

test('saving a draft merges only edited options with newer settings from another tab', async ({ page, context }) => {
  await page.goto('/fixture/');
  await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: false, unrelated: 'keep' })));
  await page.reload();
  const first = await openWatcherSettings(page);
  await first.getByLabel('Thread Watcher', { exact: true }).check();
  const other = await context.newPage();
  await other.goto('/fixture/');
  await saveWatcherSettings(other, { threadAutoWatcher: true });
  const loaded = page.waitForEvent('load');
  await first.getByRole('button', { name: 'Save Settings', exact: true }).click();
  await loaded;
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toEqual({ threadWatcher: true, threadAutoWatcher: true, unrelated: 'keep' });
  await expect(other.locator('#threadWatcher')).toBeVisible();
});

for (const failure of ['storage', 'writes', 'locks']) {
  test(`settings remain usable in this tab when ${failure} are unavailable`, async ({ page, context }) => {
    await context.addInitScript(failure => {
      if (failure === 'locks') Object.defineProperty(navigator, 'locks', { value: undefined });
      else for (const method of failure === 'writes' ? ['setItem'] : ['getItem', 'setItem', 'removeItem']) {
        Object.defineProperty(Storage.prototype, method, { value() { throw new DOMException('Unavailable', 'SecurityError'); } });
      }
    }, failure);
    await page.goto('/fixture/');
    let navigations = 0;
    page.on('request', request => { if (request.isNavigationRequest()) navigations++; });
    await saveWatcherSettings(page, { threadWatcher: true, threadAutoWatcher: true, fixedThreadWatcher: true }, { reload: false });
    await expect(page.locator('#threadWatcher')).toBeVisible();
    await expect(page.locator('#threadWatcher')).toHaveCSS('position', 'fixed');
    await expect(page.locator('input[name=awt]')).toHaveValue('1');
    await expect(page.locator('.watcherNotice')).toContainText('Changes stay in this tab');
    const dialog = await openWatcherSettings(page);
    await expect(dialog.getByLabel('Automatically watch threads you create', { exact: true })).toBeChecked();
    await dialog.getByRole('button', { name: 'Close settings' }).click();
    await saveWatcherSettings(page, { threadWatcher: false }, { reload: false });
    await expect(page.locator('#threadWatcher')).toBeHidden();
    expect(navigations).toBe(0);
  });
}

test('mobile TW opens and closes the enabled panel without disabling watched state', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/fixture/');
  await expect(page.locator('#settingsWindowLink')).toBeHidden();
  await expect(page.locator('#settingsWindowLinkMobile')).toBeVisible();
  const dialog = await openWatcherSettings(page);
  await expect(dialog.getByLabel('Pin Thread Watcher to the page', { exact: true })).toBeHidden();
  await dialog.getByRole('button', { name: 'Close settings' }).click();
  await saveWatcherSettings(page, { threadWatcher: true });
  await expect(page.locator('#threadWatcher')).toBeHidden();
  await page.locator('#watcher-open-mobile').click();
  await expect(page.locator('#threadWatcher')).toBeVisible();
  await expect(page.locator('#threadWatcher')).toHaveCSS('position', 'absolute');
  const bounds = await page.locator('#threadWatcher').boundingBox();
  expect(bounds.x).toBe(0);
  expect(bounds.width).toBeLessThanOrEqual(390);
  await page.locator('#twClose').click();
  await expect(page.locator('#threadWatcher')).toBeHidden();
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).threadWatcher)).toBe(true);
  await page.locator('#watcher-open-mobile').click();
  await expect(page.locator('#threadWatcher')).toBeVisible();
});

test('no-JavaScript pages retain their working style preference link without inert settings controls', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  try {
    const page = await context.newPage();
    await page.goto('http://127.0.0.1:3000/fixture/');
    await expect(page.locator('[data-native-settings-ready], #thread-watcher-enable')).toHaveCount(0);
    await expect(page.locator('#settingsWindowLink, #settingsWindowLinkBot, #settingsWindowLinkMobile')).toHaveCount(3);
    await expect(page.locator('#settingsWindowLink')).toHaveAttribute('href', '/settings/theme?worksafe=true');
    await page.getByRole('link', { name: 'Style', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Style preference', exact: true })).toBeVisible();
    await expect(page.locator('#theme-choice')).toBeVisible();
  } finally { await context.close(); }
});


test('native categories keep independent drafts, cancel restores focus, and cross-category saves persist', async ({ page }) => {
  const initial = {
    quotePreview: false, threadWatcher: false, hideStubs: false,
    topPageNav: false, noPictures: false, linkify: false, unrelated: 'keep',
  };
  const changes = Object.fromEntries(Object.keys(initial).filter(key => key !== 'unrelated').map(key => [key, true]));
  await page.goto('/fixture/');
  await page.evaluate(initial => localStorage.setItem('4chan-settings', JSON.stringify(initial)), initial);
  await page.reload();
  await watcherSettingsOpener(page).click();
  let dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
  const quotes = dialog.getByRole('button', { name: 'Quotes & Replying', exact: true });
  await expect(quotes).toBeFocused();
  await expect(dialog.locator('.settings-expand[aria-expanded="true"]')).toHaveCount(0);
  await (await openSettingControl(dialog, 'quotePreview')).check();
  await expect(dialog.locator('.settings-expand[aria-expanded="true"]')).toHaveCount(1);
  await expect(quotes).toHaveAttribute('aria-expanded', 'true');
  for (const key of Object.keys(changes).slice(1)) await (await openSettingControl(dialog, key)).check();
  await quotes.click();
  await expect(dialog.locator('#setting-quotePreview')).toBeHidden();
  await quotes.click();
  await expect(dialog.locator('#setting-quotePreview')).toBeChecked();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(watcherSettingsOpener(page)).toBeFocused();
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toEqual(initial);

  await saveWatcherSettings(page, changes);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toEqual({ ...initial, ...changes });
  dialog = await openWatcherSettings(page);
  for (const key of Object.keys(changes)) await expect(await openSettingControl(dialog, key)).toBeChecked();
  await dialog.getByRole('button', { name: 'Close settings', exact: true }).click();
  await expect(watcherSettingsOpener(page)).toBeFocused();
});

// Independent source Config defaults: extension.js:8795–8847. Keep this exact
// finite record assertion so opening cannot persist internal implementation state.
const firstRunDefaults = {
  quotePreview: true, backlinks: true, quickReply: true, threadUpdater: true, threadHiding: true,
  alwaysAutoUpdate: false, topPageNav: false, threadWatcher: false, threadAutoWatcher: false,
  imageExpansion: true, fitToScreenExpansion: false, threadExpansion: true, alwaysDepage: false,
  localTime: true, stickyNav: false, keyBinds: false, inlineQuotes: false, filter: false,
  revealSpoilers: false, imageHover: false, threadStats: true, IDColor: true, noPictures: false,
  embedYouTube: true, embedSoundCloud: false, updaterSound: false, customCSS: false,
  autoScroll: false, hideStubs: false, compactThreads: false, centeredThreads: false,
  dropDownNav: false, autoHideNav: false, classicNav: false, fixedThreadWatcher: false,
  persistentQR: false, forceHTTPS: false, darkTheme: false, linkify: false, unmuteWebm: false,
  disableAll: false,
};

for (const { name, raw, firstRun } of [
  { name: 'absent', raw: null, firstRun: true },
  { name: 'empty string', raw: '', firstRun: true },
  { name: 'stored empty object', raw: '{}', firstRun: false },
]) {
  test(`native Settings captures ${name} startup disclosure while reopening reads fresh preferences`, async ({ page, context }) => {
    await page.setViewportSize({ width: 1000, height: 800 });
    await page.addInitScript(raw => {
      if (sessionStorage.getItem('settings-startup-seeded')) return;
      sessionStorage.setItem('settings-startup-seeded', 'true');
      if (raw === null) localStorage.removeItem('4chan-settings');
      else localStorage.setItem('4chan-settings', raw);
    }, raw);
    await page.goto('/fixture/');
    expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(raw);
    await watcherSettingsOpener(page).click();
    let dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
    const expanded = () => dialog.locator('.settings-expand[aria-expanded="true"]');
    await expect(expanded()).toHaveCount(firstRun ? 6 : 0);
    if (firstRun) {
      await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings') || 'null'))).toEqual(firstRunDefaults);
    } else {
      expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(raw);
    }
    await (await openSettingControl(dialog, 'linkify')).uncheck();

    const other = await context.newPage();
    try {
      await other.goto('/fixture/');
      await page.evaluate(() => {
        window.settingsStorageChange = new Promise(resolve => window.addEventListener('storage', resolve, { once: true }));
      });
      await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ linkify: true })));
      await page.evaluate(() => window.settingsStorageChange);
      // A live draft stays intact; close/reopen picks up current preference values.
      await expect(dialog.getByLabel('Linkify URLs', { exact: true })).not.toBeChecked();
      await dialog.getByRole('button', { name: 'Miscellaneous', exact: true }).click();
      await expect(dialog.getByRole('button', { name: 'Miscellaneous', exact: true })).toHaveAttribute('aria-expanded', 'false');
      await page.keyboard.press('Escape');
      await watcherSettingsOpener(page).click();
      dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
      await expect(expanded()).toHaveCount(firstRun ? 6 : 0);
      await expect(await openSettingControl(dialog, 'linkify')).toBeChecked();
      await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings'))))
        .toEqual(firstRun ? { ...firstRunDefaults, linkify: true } : { linkify: true });
      await page.keyboard.press('Escape');

      await page.evaluate(() => {
        window.settingsStorageChange = new Promise(resolve => window.addEventListener('storage', resolve, { once: true }));
      });
      await other.evaluate(() => localStorage.removeItem('4chan-settings'));
      await page.evaluate(() => window.settingsStorageChange);
      await watcherSettingsOpener(page).click();
      dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
      await expect(expanded()).toHaveCount(firstRun ? 6 : 0);
      await expect(await openSettingControl(dialog, 'linkify')).not.toBeChecked();
      if (firstRun) {
        await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings') || 'null'))).toEqual(firstRunDefaults);
      } else {
        expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBeNull();
      }
      await page.keyboard.press('Escape');

      // A new document recaptures persisted initialization or remaining absence.
      await page.reload();
      expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings'))))
        .toEqual(firstRun ? firstRunDefaults : null);
      await watcherSettingsOpener(page).click();
      dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
      await expect(expanded()).toHaveCount(firstRun ? 0 : 6);
      await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings') || 'null'))).toEqual(firstRunDefaults);
    } finally { await other.close(); }
  });
}

for (const action of ['close', 'save', 'newer-storage']) {
  test(`first-open initialization under the shared lock respects ${action}`, async ({ page, context }) => {
    await page.goto('/fixture/');
    const other = await context.newPage();
    await other.goto('/');
    await other.evaluate(() => new Promise(resolve => {
      window.initializationLockDone = navigator.locks.request('paperboard-thread-watcher', async () => {
        resolve();
        await new Promise(release => { window.releaseInitializationLock = release; });
      });
    }));
    try {
      await page.evaluate(() => {
        const setItem = Storage.prototype.setItem;
        sessionStorage.setItem('settings-initialization-writes', '[]');
        Storage.prototype.setItem = function (key, value) {
          if (this === localStorage && key === '4chan-settings') {
            const writes = JSON.parse(sessionStorage.getItem('settings-initialization-writes'));
            writes.push(JSON.parse(value));
            setItem.call(sessionStorage, 'settings-initialization-writes', JSON.stringify(writes));
          }
          return setItem.call(this, key, value);
        };
      });
      await watcherSettingsOpener(page).click();
      const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
      await expect.poll(() => page.evaluate(async () => (await navigator.locks.query()).pending
        .filter(lock => lock.name === 'paperboard-thread-watcher').length)).toBe(1);
      expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBeNull();
      await expect(dialog.getByRole('button', { name: 'Export Settings', exact: true })).toBeDisabled();
      const newer = { quotePreview: false, linkify: true, customMenuList: 'fixture demo',
        'SN-position': 'top: 24px; left: 8px;', unrelated: { retained: true } };
      let loaded;
      if (action === 'close') {
        await page.keyboard.press('Escape');
        await expect(dialog).toHaveCount(0);
      } else if (action === 'save') {
        await (await openSettingControl(dialog, 'linkify')).check();
        loaded = page.waitForEvent('load');
        await dialog.getByRole('button', { name: 'Save Settings', exact: true }).click();
        await expect(dialog.getByRole('button', { name: 'Save Settings', exact: true })).toBeDisabled();
      } else {
        await other.evaluate(newer => localStorage.setItem('4chan-settings', JSON.stringify(newer)), newer);
      }
      await other.evaluate(async () => { window.releaseInitializationLock(); await window.initializationLockDone; });
      if (loaded) await loaded;
      else await page.evaluate(() => navigator.locks.request('paperboard-thread-watcher', () => {}));
      const expected = action === 'close' ? null : { ...firstRunDefaults, ...(action === 'save' ? { linkify: true } : newer) };
      expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toEqual(expected);
      expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('settings-initialization-writes'))))
        .toEqual(action === 'close' ? [] : [expected]);
      if (action === 'newer-storage') {
        await expect(dialog.getByRole('button', { name: 'Export Settings', exact: true })).toBeEnabled();
        // A cross-tab change is persisted but never replaces the user's open draft.
        await expect(await openSettingControl(dialog, 'quotePreview')).toBeChecked();
      }
    } finally { await other.close(); }
  });
}

for (const failure of ['malformed', 'oversized', 'unavailable']) {
  test(`first-open initialization leaves ${failure} fresh storage untouched after waiting for the lock`, async ({ page, context }) => {
    await page.goto('/fixture/');
    const other = await context.newPage();
    await other.goto('/');
    await other.evaluate(() => new Promise(resolve => {
      window.invalidStartupLockDone = navigator.locks.request('paperboard-thread-watcher', async () => {
        resolve();
        await new Promise(release => { window.releaseInvalidStartupLock = release; });
      });
    }));
    try {
      const dialog = await openWatcherSettings(page);
      await expect.poll(() => page.evaluate(async () => (await navigator.locks.query()).pending
        .filter(lock => lock.name === 'paperboard-thread-watcher').length)).toBe(1);
      const raw = failure === 'malformed' ? '{' : failure === 'oversized'
        ? JSON.stringify({ unrelated: 'x'.repeat(4096) }) : null;
      if (raw !== null) await other.evaluate(raw => localStorage.setItem('4chan-settings', raw), raw);
      else await page.evaluate(() => {
        const getItem = Storage.prototype.getItem;
        window.restoreStartupStorageRead = () => { Storage.prototype.getItem = getItem; };
        Storage.prototype.getItem = function (key) {
          if (this === localStorage && key === '4chan-settings') throw new DOMException('Owned storage denial', 'SecurityError');
          return getItem.call(this, key);
        };
      });
      await other.evaluate(async () => { window.releaseInvalidStartupLock(); await window.invalidStartupLockDone; });
      await page.evaluate(() => navigator.locks.request('paperboard-thread-watcher', () => {}));
      await expect(dialog.getByRole('button', { name: 'Export Settings', exact: true })).toBeEnabled();
      if (failure === 'unavailable') await page.evaluate(() => window.restoreStartupStorageRead());
      expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(raw);
      await page.keyboard.press('Escape');
    } finally { await other.close(); }
  });
}

for (const failure of ['quota', 'missing-locks', 'denied-locks']) {
  test(`first-open ${failure} failure keeps defaults usable without claiming persistence`, async ({ page }) => {
    await page.addInitScript(failure => {
      window.startupSettingsWrites = 0;
      window.startupSettingsSaved = 0;
      document.addEventListener('4chanSettingsSaved', () => { window.startupSettingsSaved++; });
      const setItem = Storage.prototype.setItem;
      Storage.prototype.setItem = function (key, value) {
        if (this === localStorage && key === '4chan-settings') {
          window.startupSettingsWrites++;
          if (failure === 'quota') throw new DOMException('Owned quota failure', 'QuotaExceededError');
        }
        return setItem.call(this, key, value);
      };
      if (failure === 'missing-locks') Object.defineProperty(navigator, 'locks', { value: undefined });
      else if (failure === 'denied-locks') Object.defineProperty(navigator.locks, 'request', {
        configurable: true,
        value: async () => { throw new DOMException('Owned lock denial', 'SecurityError'); },
      });
    }, failure);
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto('/fixture/');
    let navigations = 0, loads = 0;
    page.on('request', request => { if (request.isNavigationRequest()) navigations++; });
    page.on('load', () => { loads++; });
    expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBeNull();
    expect(await page.evaluate(() => window.startupSettingsWrites)).toBe(0);

    let dialog = await openWatcherSettings(page);
    const tabOnly = 'Settings are available only in this tab because browser storage is unavailable.';
    await expect(dialog.getByRole('status')).toHaveText(tabOnly);
    await expect(dialog.getByRole('button', { name: 'Export Settings', exact: true })).toBeEnabled();
    await expect(await openSettingControl(dialog, 'quotePreview')).toBeChecked();
    await expect(await openSettingControl(dialog, 'threadWatcher')).not.toBeChecked();
    await (await openSettingControl(dialog, 'quotePreview')).uncheck();
    await expect(await openSettingControl(dialog, 'quotePreview')).not.toBeChecked();
    await dialog.getByRole('button', { name: 'Export Settings', exact: true }).click();
    const exported = page.getByRole('dialog', { name: 'Export Settings', exact: true });
    const url = await exported.getByLabel('Settings export URL', { exact: true }).inputValue();
    const payload = JSON.parse(decodeURIComponent(new URL(url).hash.slice(5)));
    // Export remains useful with volatile defaults and excludes unsaved edits.
    expect(JSON.parse(payload.settings)).toEqual(firstRunDefaults);
    await exported.getByRole('button', { name: 'Close', exact: true }).click();
    await page.keyboard.press('Escape');
    dialog = await openWatcherSettings(page);
    await expect(dialog.getByRole('status')).toHaveText(tabOnly);
    await expect(dialog.getByRole('button', { name: 'Export Settings', exact: true })).toBeEnabled();
    await expect(await openSettingControl(dialog, 'quotePreview')).toBeChecked();
    expect(await page.evaluate(() => ({
      raw: localStorage.getItem('4chan-settings'),
      writes: window.startupSettingsWrites,
      saved: window.startupSettingsSaved,
    }))).toEqual({ raw: null, writes: failure === 'quota' ? 1 : 0, saved: 0 });
    expect(navigations).toBe(0);
    expect(loads).toBe(0);
    expect(errors).toEqual([]);
  });
}
