import { openNativeSettingsCategory, watcherSettingsOpener } from './helpers/watcher-settings.js';
import { test, expect } from '@playwright/test';

const settingsKey = '4chan-settings';

test('catalog Settings uses the actual board directory and disables navigation without reload', async ({ page }) => {
  const errors = [], unexpected = [], directoryRequests = []; page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (new URL(request.url()).origin !== 'http://127.0.0.1:3000') unexpected.push(request.url()); });
  await page.addInitScript(() => localStorage.setItem('4chan-settings', '{}'));
  page.on('request', request => { if (new URL(request.url()).pathname === '/_watch/boards') directoryRequests.push(request.url()); });
  const directory = await page.request.get('/_watch/boards'); expect(directory.status()).toBe(200);
  await page.goto('/fixture/catalog');
  const bar = page.getByRole('navigation', { name: 'Persistent board navigation', exact: true });
  await expect(bar).toBeVisible(); await expect(bar.getByLabel('Board', { exact: true })).toHaveValue('fixture');
  await expect(bar.locator('option[value="demo"]')).toHaveCount(1);
  expect(await bar.locator('option').evaluateAll(options => options.map(option => option.value))).toEqual((await directory.json()).boards.map(entry => entry.board));
  let navigations = 0; page.on('framenavigated', () => navigations++);
  await bar.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.locator('#theme-ddn').uncheck(); await page.locator('#theme-save').click();
  await expect(page.locator('#theme')).toBeHidden(); await expect(bar).toHaveCount(0);
  expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key)), settingsKey)).toEqual({ threadWatcher: false, dropDownNav: false });
  await expect(page.locator('#settingsWindowLink')).toBeVisible(); expect(navigations).toBe(0);
  expect(errors).toEqual([]); expect(unexpected).toEqual([]); expect(directoryRequests).toEqual([]);
});

test('persistent board navigation saves settings, uses the actual directory and supports keyboard relocation', async ({ page, context }) => {
  const failures = [], unexpected = [];
  page.on('pageerror', error => failures.push(error.message));
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.origin !== 'http://127.0.0.1:3000') unexpected.push(url.origin);
  });
  await page.goto('/fixture/0');
  await watcherSettingsOpener(page).click();
  const settings = page.getByRole('dialog', { name: 'Settings', exact: true });
  await openNativeSettingsCategory(settings, 'Navigation');
  for (const name of ['Use persistent drop-down navigation bar', 'Page navigation at top of page', 'Navigation arrows']) {
    await settings.getByLabel(name, { exact: true }).check();
  }
  await Promise.all([page.waitForEvent('load'), settings.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
  const bar = page.getByRole('navigation', { name: 'Persistent board navigation', exact: true });
  await expect(bar).toBeVisible();
  await expect(bar.getByLabel('Board', { exact: true })).toHaveValue('fixture');
  await expect(bar.locator('option[value="demo"]')).toHaveCount(1);
  await expect(page.getByRole('navigation', { name: 'Top page navigation', exact: true })).toBeVisible();
  const arrows = page.getByRole('navigation', { name: 'Page navigation arrows', exact: true });
  await expect(arrows).toBeVisible();
  await expect(arrows.locator('img')).toHaveCount(2);
  expect(await arrows.locator('img').evaluateAll(images => images.every(image => image.complete && image.naturalWidth === 18))).toBe(true);

  const handle = arrows.locator(':scope > div');
  const before = await arrows.boundingBox();
  await handle.focus(); await page.keyboard.press('Shift+ArrowDown');
  await expect.poll(() => page.evaluate(key => JSON.parse(localStorage.getItem(key))['SN-position'], settingsKey)).toMatch(/top:/);
  expect((await arrows.boundingBox()).y).toBeGreaterThan(before.y);
  const saved = await page.evaluate(key => JSON.parse(localStorage.getItem(key))['SN-position'], settingsKey);
  await page.reload();
  await expect(arrows).toBeVisible();
  expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key))['SN-position'], settingsKey)).toBe(saved);

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(bar).toHaveCount(0);
  expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key)).dropDownNav, settingsKey)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.setViewportSize({ width: 1000, height: 800 });
  await expect(bar).toBeVisible();
  await bar.getByLabel('Board', { exact: true }).selectOption('demo');
  await expect(page).toHaveURL('http://127.0.0.1:3000/demo/');
  await expect(bar.getByLabel('Board', { exact: true })).toHaveValue('demo');

  const other = await context.newPage();
  try {
    await other.goto('/');
    await other.evaluate(key => {
      const settings = JSON.parse(localStorage.getItem(key));
      localStorage.setItem(key, JSON.stringify({ ...settings, disableAll: true }));
    }, settingsKey);
    await expect(bar).toHaveCount(0);
    await expect(arrows).toHaveCount(0);
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page.getByRole('navigation', { name: 'Mobile board navigation', exact: true })).toBeVisible();
    await expect(page.locator('#boardSelectMobile')).toHaveValue('demo');
    await expect(page.locator('#boardNavDesktop, #boardNavDesktopFoot')).toHaveCount(2);
    await expect(page.locator('#boardNavDesktop')).toBeHidden();
  } finally { await other.close(); }
  expect(failures).toEqual([]); expect(unexpected).toEqual([]);
});

test('actual Shift-drag retains viewport bounds and does not activate an arrow click', async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ stickyNav: true, threadStats: false }));
  });
  await page.goto('/demo/');
  const nav = page.locator('#stickyNav');
  await expect(nav).toBeVisible();
  const originalScroll = await page.evaluate(() => scrollY);
  const box = await nav.locator('button').first().boundingBox();
  await page.keyboard.down('Shift');
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(100, 160, { steps: 8 });
  await page.mouse.up();
  await page.keyboard.up('Shift');
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings'))['SN-position'])).toMatch(/top:/);
  expect(await page.evaluate(() => scrollY)).toBe(originalScroll);
  const after = await nav.boundingBox();
  expect(after.x).toBeGreaterThanOrEqual(0); expect(after.y).toBeGreaterThanOrEqual(0);
  expect(after.x + after.width).toBeLessThanOrEqual(1280);
  expect(after.y + after.height).toBeLessThanOrEqual(900);
});

test('mounted navigation arrows follow runtime device-density changes', async ({ page }) => {
  await page.addInitScript(() => {
    const browserMatchMedia = window.matchMedia.bind(window);
    let highDensity = false;
    const densityEvents = new EventTarget();
    const densityQuery = {
      media: '(min-resolution: 2dppx)',
      get matches() { return highDensity; },
      addEventListener: (...args) => densityEvents.addEventListener(...args),
      removeEventListener: (...args) => densityEvents.removeEventListener(...args),
      addListener: listener => densityEvents.addEventListener('change', listener),
      removeListener: listener => densityEvents.removeEventListener('change', listener),
    };
    window.matchMedia = query => query === densityQuery.media ? densityQuery : browserMatchMedia(query);
    window.setNavigationDensity = value => {
      highDensity = value === true;
      const event = new Event('change');
      Object.defineProperty(event, 'matches', { value: highDensity });
      densityEvents.dispatchEvent(event);
    };
    localStorage.setItem('4chan-settings', JSON.stringify({ stickyNav: true, threadStats: false }));
  });
  await page.goto('/demo/');
  const image = page.locator('#stickyNav button').first().locator('img');
  await expect(image).toHaveAttribute('src', /\/arrow_up\.png$/);
  await page.evaluate(() => setNavigationDensity(true));
  await expect(image).toHaveAttribute('src', /\/arrow_up@2x\.png$/);
  await page.evaluate(() => setNavigationDensity(false));
  await expect(image).toHaveAttribute('src', /\/arrow_up\.png$/);
});

for (const mobileDevice of [false, true]) {
  for (const width of [390, 1000]) {
    for (const neverMobile of [false, true]) {
      test(`first-run navigation separates mobile UA=${mobileDevice}, width=${width}, never-mobile=${neverMobile}`, async ({ browser }) => {
        const context = await browser.newContext({
          viewport: { width, height: 844 },
          userAgent: mobileDevice
            ? 'Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Mobile Safari/537.36'
            : 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/130.0.0.0 Safari/537.36',
        });
        try {
          await context.addInitScript(neverMobile => {
            if (neverMobile) localStorage.setItem('4chan_never_show_mobile', 'true');
          }, neverMobile);
          const page = await context.newPage();
          await page.goto('http://127.0.0.1:3000/fixture/');
          const mobileLayout = width <= 480 && !neverMobile;
          const bar = page.getByRole('navigation', { name: 'Persistent board navigation', exact: true });
          await expect(watcherSettingsOpener(page)).toBeVisible();
          await expect(bar).toHaveCount(mobileDevice && !mobileLayout ? 1 : 0);
          expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBeNull();

          await watcherSettingsOpener(page).click();
          const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
          await expect(dialog.locator('.settings-expand[aria-expanded="true"]')).toHaveCount(6);
          await expect.poll(() => page.evaluate(() => {
            const raw = localStorage.getItem('4chan-settings');
            if (!raw) return null;
            const { dropDownNav, topPageNav, embedYouTube, linkify, compactThreads } = JSON.parse(raw);
            return { dropDownNav, topPageNav, embedYouTube, linkify, compactThreads };
          })).toEqual({
            dropDownNav: mobileDevice, topPageNav: false,
            embedYouTube: !mobileLayout, linkify: mobileLayout, compactThreads: false,
          });
          const persisted = await page.evaluate(() => localStorage.getItem('4chan-settings'));
          await page.keyboard.press('Escape');
          await page.reload();
          await expect(bar).toHaveCount(mobileDevice && !mobileLayout ? 1 : 0);
          await watcherSettingsOpener(page).click();
          await expect(page.locator('.settings-expand[aria-expanded="true"]')).toHaveCount(0);
          expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(persisted);
        } finally { await context.close(); }
      });
    }
  }
}

test('returning mobile-UA users retain explicit desktop-layout navigation preferences', async ({ browser }) => {
  const context = await browser.newContext({
    viewport: { width: 1000, height: 844 },
    userAgent: 'Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Mobile Safari/537.36',
  });
  try {
    const saved = JSON.stringify({ dropDownNav: false, topPageNav: true, unrelated: 'keep' });
    await context.addInitScript(saved => localStorage.setItem('4chan-settings', saved), saved);
    const page = await context.newPage();
    await page.goto('http://127.0.0.1:3000/fixture/0');
    await expect(page.getByRole('navigation', { name: 'Persistent board navigation', exact: true })).toHaveCount(0);
    await expect(page.getByRole('navigation', { name: 'Top page navigation', exact: true })).toBeVisible();
    await watcherSettingsOpener(page).click();
    const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
    await expect(dialog.locator('.settings-expand[aria-expanded="true"]')).toHaveCount(0);
    await openNativeSettingsCategory(dialog, 'Navigation');
    await expect(dialog.getByLabel('Use persistent drop-down navigation bar', { exact: true })).not.toBeChecked();
    await expect(dialog.getByLabel('Page navigation at top of page', { exact: true })).toBeChecked();
    expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(saved);
  } finally { await context.close(); }
});
