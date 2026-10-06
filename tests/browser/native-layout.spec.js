import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { watcherSettingsOpener } from './helpers/watcher-settings.js';
import { test as base, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-native-layout-password';
    const write = form => withPostingHistory(() => request.post('/demo/post', {
      headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password },
    }));
    const created = await write({ resto: '0', sub: 'Owned native layout', com: 'Original layout post' });
    expect(created.status()).toBe(303);
    const id = created.headers().location.match(/thread\/(\d+)/)[1];
    try {
      const reply = await write({ resto: id, com: `>>${id}\nOwned layout reply` });
      expect(reply.status()).toBe(303);
      await use({ id, url: `/demo/thread/${id}` });
    } finally {
      await withDeletionQuota(async () => {
        const removed = await request.post('/demo/delete', {
          headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
        });
        expect(removed.status()).toBe(303);
      });
    }
  },
});

const themeLink = page => page.locator('link[data-native-theme-stylesheet]');
const layout = page => page.locator('body').getAttribute('data-native-thread-layout');
const watcherFamily = page => page.evaluate(() => getComputedStyle(document.documentElement)
  .getPropertyValue('--watcher-icon-family').trim());

async function expectThemeRuntime(page, id, family) {
  await expect.poll(() => page.evaluate(() => window.boardThemeFamilies?.at(-1))).toBe(family);
  await expect.poll(() => watcherFamily(page)).toBe(family);
  await expect(page.locator('#twPrune img')).toHaveAttribute('src', `/static/watcher/${family}/refresh.png`);
  await expect(page.locator('#stickyNav button').first().locator('img')).toHaveAttribute('src', `/static/navigation/${family}/arrow_up.png`);
  await expect(page.locator(`#p${id} [data-post-menu]`)).toHaveAttribute('data-family', family);
  await expect(page.locator(`#sa${id} img`)).toHaveAttribute('src', `/static/watcher/${family}/post_expand_minus.png`);
  await expect(page.locator(`#bl_${id}`)).toHaveAttribute('data-backlink-family', family);
}

async function openSettings(page) {
  await watcherSettingsOpener(page).click();
  const dialog = page.locator('#settingsMenu');
  await expect(dialog).toBeVisible();
  const expand = dialog.locator('#settings-expand-all');
  if (await expand.count()) await expand.click();
  return dialog;
}

test('native layout settings preserve source precedence across desktop, mobile and cross-tab disableAll', async ({ page, context, owned }) => {
  await page.goto(owned.url);
  const dialog = await openSettings(page);
  const compact = dialog.getByLabel('Force long posts to wrap');
  const centered = dialog.getByLabel('Center threads');
  const dark = dialog.getByLabel('Use a dark theme');
  await expect(compact).toBeVisible();
  await expect(centered).toBeVisible();
  await expect(dark).toBeVisible();
  await compact.check();
  await centered.check();
  await dark.check();
  const navigation = page.waitForEvent('framenavigated');
  await dialog.getByRole('button', { name: 'Save Settings' }).click();
  await navigation;

  await expect.poll(() => layout(page)).toBe('compact');
  await expect.poll(() => page.locator('.thread').evaluate(node => getComputedStyle(node).maxWidth)).toBe('75%');
  await expect(themeLink(page)).toHaveAttribute('href', /(?:\?|&)theme=tomorrow(?:&|$)/);
  await expect.poll(() => watcherFamily(page)).toBe('tomorrow');

  let reopened = await openSettings(page);
  await expect(reopened.getByLabel('Force long posts to wrap')).toBeChecked();
  await expect(reopened.getByLabel('Center threads')).toBeChecked();
  await expect(reopened.getByLabel('Use a dark theme')).toBeChecked();
  await reopened.getByRole('button', { name: 'Close settings' }).click();

  await page.setViewportSize({ width: 390, height: 800 });
  await expect.poll(() => layout(page)).toBe('centered');
  await expect.poll(() => page.locator('.sideArrows').first().evaluate(node => getComputedStyle(node).display)).toBe('none');

  await page.evaluate(() => {
    localStorage.setItem('4chan_never_show_mobile', 'true');
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile' }));
  });
  await expect.poll(() => layout(page)).toBe('compact');

  const other = await context.newPage();
  await other.goto(owned.url);
  await other.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem('4chan-settings') || '{}');
    localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, disableAll: true }));
  });
  await expect.poll(() => layout(page)).toBeNull();
  await expect(themeLink(page)).not.toHaveAttribute('href', /(?:\?|&)theme=tomorrow(?:&|$)/);
  await other.close();
});

test('darkTheme uses Tomorrow transiently while preserving the server theme request and manual style choice', async ({ page, context, owned }) => {
  await context.addCookies([
    { name: 'board-theme', value: 'photon', url: origin, httpOnly: true, sameSite: 'Lax' },
    { name: 'board-theme-ws', value: 'photon', url: origin, httpOnly: true, sameSite: 'Lax' },
  ]);
  await page.addInitScript(() => {
    window.boardThemeFamilies = [];
    document.addEventListener('boardThemeChanged', () => {
      boardThemeFamilies.push(getComputedStyle(document.documentElement).getPropertyValue('--watcher-icon-family').trim());
    });
    if (sessionStorage.getItem('native-layout-theme-seeded') !== 'true') {
      sessionStorage.setItem('native-layout-theme-seeded', 'true');
      localStorage.setItem('4chan-settings', JSON.stringify({
        darkTheme: true, threadWatcher: true, stickyNav: true, noPictures: true,
      }));
    }
  });
  await page.goto('/demo/');
  await expect(themeLink(page)).toHaveAttribute('href', /(?:\?|&)theme=tomorrow(?:&|$)/);
  await expect.poll(() => watcherFamily(page)).toBe('tomorrow');
  await expectThemeRuntime(page, owned.id, 'tomorrow');
  let cookies = await context.cookies(origin);
  expect(cookies.find(cookie => cookie.name === 'board-theme')?.value).toBe('photon');
  expect(cookies.find(cookie => cookie.name === 'board-theme-ws')?.value).toBe('photon');

  const other = await context.newPage();
  await other.goto('/demo/');
  await other.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem('4chan-settings') || '{}');
    localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, darkTheme: false }));
  });
  await expect(themeLink(page)).not.toHaveAttribute('href', /(?:\?|&)theme=tomorrow(?:&|$)/);
  await expectThemeRuntime(page, owned.id, 'photon');

  await other.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem('4chan-settings') || '{}');
    localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, darkTheme: true }));
  });
  await expectThemeRuntime(page, owned.id, 'tomorrow');
  await other.close();

  await Promise.all([
    page.waitForURL(/\/settings\/theme\?worksafe=(?:true|false)$/),
    page.getByRole('link', { name: 'Style' }).click(),
  ]);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings') || '{}').darkTheme)).toBe(false);
  await expect(page.locator('#theme-choice')).toHaveValue('photon');
  await expect.poll(() => watcherFamily(page)).toBe('photon');
  cookies = await context.cookies(origin);
  expect(cookies.find(cookie => cookie.name === 'board-theme')?.value).toBe('photon');
  expect(cookies.find(cookie => cookie.name === 'board-theme-ws')?.value).toBe('photon');
});

test('no-JavaScript board rendering keeps the ordinary server theme and thread layout', async ({ browser, owned }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  try {
    await page.goto(`${origin}${owned.url}`);
    await expect(page.locator(`#t${owned.id}`)).toBeVisible();
    await expect(page.locator('body')).not.toHaveAttribute('data-native-thread-layout');
    await expect(themeLink(page)).not.toHaveAttribute('href', /(?:\?|&)theme=tomorrow(?:&|$)/);
  } finally { await context.close(); }
});
