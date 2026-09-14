import { test, expect } from '@playwright/test';
import { openWatcherSettings, saveWatcherSettings } from './helpers/watcher-settings.js';

test('catalog settings discard cancelled edits and save the native watcher flag without navigation', async ({ page }) => {
  await page.goto('/test/catalog');
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest()) navigations++; });
  let dialog = await openWatcherSettings(page);
  await expect(dialog).toHaveAttribute('id', 'theme');
  await expect(dialog.locator('#theme-tw')).toBeFocused();
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
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toEqual({ disableAll: false, threadWatcher: true, threadAutoWatcher: true, unrelated: 'keep' });
  expect(navigations).toBe(0);
});

test('Monitoring saves auto-watch and fixed placement; Disable overrides checked options', async ({ page }) => {
  await page.goto('/test/');
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
  await page.goto('/test/');
  await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: false, unrelated: 'keep' })));
  const first = await openWatcherSettings(page);
  await first.getByLabel('Thread Watcher', { exact: true }).check();
  const other = await context.newPage();
  await other.goto('/test/');
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
    await page.goto('/test/');
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
  await page.goto('/test/');
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
    await page.goto('http://127.0.0.1:3000/test/');
    await expect(page.locator('#settingsWindowLink, #settingsWindowLinkMobile, #thread-watcher-enable')).toHaveCount(0);
    await page.getByRole('link', { name: 'Style', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Style preference', exact: true })).toBeVisible();
    await expect(page.locator('#theme-choice')).toBeVisible();
  } finally { await context.close(); }
});
