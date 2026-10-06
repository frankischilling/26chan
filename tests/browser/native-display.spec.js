import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';
import { openNativeSettingsCategory, watcherSettingsOpener } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';

async function openSettings(page, category = 'Navigation') {
  await watcherSettingsOpener(page).click();
  const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
  await openNativeSettingsCategory(dialog, category);
  return dialog;
}

const customMenu = page => page.locator('#boardNavDesktop .customBoardList');
async function expectCustomMenus(page, visible = true) {
  await expect(page.locator('.customBoardList')).toHaveCount(2);
  for (const parent of ['#boardNavDesktop', '#boardNavDesktopFoot']) {
    if (visible) await expect(page.locator(`${parent} .customBoardList`)).toBeVisible();
    else await expect(page.locator(`${parent} .customBoardList`)).toBeHidden();
  }
}

test('custom navigation persists and synchronizes across tabs while mobile keeps the native board selector', async ({ page, context }) => {
  await page.goto('/fixture/');
  let settings = await openSettings(page);
  await settings.locator('#custom-menu-edit').click();
  const editor = page.getByRole('dialog', { name: 'Custom Board List', exact: true });
  await editor.getByLabel('Boards', { exact: true }).fill('demo fixture');
  await editor.getByRole('button', { name: 'Save board list', exact: true }).click();
  await expect(editor).toHaveCount(0);
  await settings.getByRole('button', { name: 'Close settings' }).click();
  await expectCustomMenus(page);
  await expect(page.locator('.customBoardList a').first()).toHaveAttribute('href', '/demo/');
  await page.reload();
  await expectCustomMenus(page);
  const other = await context.newPage();
  try {
    await other.goto('/fixture/');
    await expectCustomMenus(other);
    await customMenu(page).getByRole('link', { name: 'Edit', exact: true }).click();
    await editor.getByLabel('Boards', { exact: true }).fill('fixture demo');
    await editor.getByRole('button', { name: 'Save board list' }).click();
    await expect(other.locator('.customBoardList a').first()).toHaveAttribute('href', '/fixture/');
    await customMenu(page).getByRole('link', { name: 'Show all boards' }).click();
    await expect(page.locator('.customBoardList')).toHaveCount(0);
    await expect(page.getByRole('navigation', { name: 'Board navigation', exact: true })).toBeVisible();
    await expectCustomMenus(other);
    await page.setViewportSize({ width: 390, height: 844 });
    await page.reload();
    await expectCustomMenus(page, false);
    await expect(page.locator('#boardNavMobile')).toBeVisible();
    await expect(page.locator('#boardSelectMobile')).toHaveValue('fixture');
    const directory = await (await context.request.get('/_watch/boards')).json();
    expect(await page.locator('#boardSelectMobile option').evaluateAll(nodes => nodes.map(node => node.value))).toEqual(directory.boards.map(board => board.board));
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    settings = await openSettings(page);
    await expect(settings.getByLabel('Custom board list', { exact: true })).toHaveCount(0);
    await expect(settings.locator('#custom-menu-edit')).toHaveCount(0);
    await settings.getByRole('button', { name: 'Close settings' }).click();
    const desktopSettings = await openSettings(other);
    await desktopSettings.getByLabel('Custom board list', { exact: true }).uncheck();
    await Promise.all([other.waitForEvent('load'), desktopSettings.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
    await expect(page.locator('.customBoardList')).toHaveCount(0);
    await expect(other.locator('.customBoardList')).toHaveCount(0);
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).customMenuList)).toBe('fixture demo');
  } finally { await other.close(); }
});

test('mobile saves retain hidden preferences and merge current cross-tab changes', async ({ page, context }) => {
  const hidden = {
    inlineQuotes: true, persistentQR: true, autoScroll: true, updaterSound: true,
    fixedThreadWatcher: true, filter: true, hideStubs: true, dropDownNav: false,
    classicNav: true, autoHideNav: true, customMenu: true, topPageNav: true,
    stickyNav: true, keyBinds: true, fitToScreenExpansion: true, imageHover: true,
    imageHoverBg: true, embedYouTube: true, embedSoundCloud: true,
    compactThreads: true, centeredThreads: true,
  };
  const initial = { ...hidden, customMenuList: 'fixture demo', localTime: true,
    noPictures: false, unrelated: 'keep' };
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/fixture/');
  await page.evaluate(value => localStorage.setItem('4chan-settings', JSON.stringify(value)), initial);
  await page.reload();
  let settings = await openSettings(page, 'Miscellaneous');
  for (const key of Object.keys(hidden)) await expect(settings.locator(`#setting-${key}`)).toHaveCount(0);
  await expect(settings.locator('#custom-menu-edit, #keybinds-open, #filters-edit')).toHaveCount(0);
  await settings.getByLabel('Convert dates to local time', { exact: true }).uncheck();
  await page.keyboard.press('Escape');
  await expect(watcherSettingsOpener(page)).toBeFocused();
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toEqual(initial);

  settings = await openSettings(page, 'Miscellaneous');
  await expect(settings.getByLabel('Convert dates to local time', { exact: true })).toBeChecked();
  await settings.getByLabel('Convert dates to local time', { exact: true }).uncheck();
  const other = await context.newPage();
  try {
    await other.setViewportSize({ width: 1280, height: 900 });
    await other.goto('/fixture/');
    // A real second tab changes both an omitted control and an untouched visible one.
    // Saving the mobile draft must merge against this state, not its opening snapshot.
    const newer = { updaterSound: false, noPictures: true, customMenuList: 'demo fixture', unrelated: 'newer' };
    await other.evaluate(changes => {
      const current = JSON.parse(localStorage.getItem('4chan-settings'));
      localStorage.setItem('4chan-settings', JSON.stringify({ ...current, ...changes }));
    }, newer);
    await expect(settings).toBeVisible();
    await expect(settings.getByLabel('Convert dates to local time', { exact: true })).not.toBeChecked();
    await Promise.all([page.waitForEvent('load'), settings.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
    const saved = { ...initial, ...newer, localTime: false };
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toEqual(saved);
    expect(await other.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')))).toEqual(saved);
    await expectCustomMenus(other);
    await expect(customMenu(other).locator('a').first()).toHaveAttribute('href', '/demo/');
    // A stored desktop-only keyboard preference still runs on mobile after saving.
    await page.getByRole('heading', { level: 1 }).click();
    await Promise.all([page.waitForURL('**/fixture/catalog'), page.keyboard.press('c')]);
  } finally { await other.close(); }
});

test('local time uses the real timestamp while settings and script-free pages preserve server output', async ({ browser, request }) => {
  const password = 'owned-display-password';
  const created = await withPostingHistory(() => request.post('/fixture/post', {
    headers: { Origin: origin }, maxRedirects: 0,
    form: { resto: '0', com: 'Owned display date.', password },
  }));
  expect(created.status()).toBe(303);
  const url = created.headers().location.split('#')[0];
  const id = url.split('/').at(-1);
  const context = await browser.newContext({ timezoneId: 'Asia/Kathmandu' });
  const plain = await browser.newContext({ javaScriptEnabled: false, timezoneId: 'Asia/Kathmandu' });
  try {
    const page = await context.newPage(), noJS = await plain.newPage();
    await noJS.goto(origin + url);
    const server = await noJS.locator(`#pi${id} time`).textContent();
    await page.goto(origin + url);
    const clock = page.locator(`#pi${id} time`);
    await expect(clock).toHaveAttribute('title', 'Timezone: UTC+5:45');
    const expected = await clock.evaluate(element => {
      const date = new Date(element.dateTime), two = n => String(n).padStart(2, '0');
      return `${two(date.getMonth() + 1)}/${two(date.getDate())}/${String(date.getFullYear()).slice(-2)}`
        + `(${['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][date.getDay()]})${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`;
    });
    await expect(clock).toHaveText(expected);
    const settings = await openSettings(page, 'Miscellaneous');
    await expect(settings.getByLabel('Convert dates to local time', { exact: true })).toBeChecked();
    await settings.getByLabel('Convert dates to local time', { exact: true }).uncheck();
    await Promise.all([page.waitForEvent('load'), settings.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
    await expect(clock).toHaveText(server);
    await expect(clock).not.toHaveAttribute('title');
    const response = await request.get(`/fixture/thread/${id}.json`);
    const data = await response.json();
    expect(typeof data.posts[0].time).toBe('number');
    expect(data.posts[0].now).toBe(server);
  } finally {
    await context.close(); await plain.close();
    await withDeletionQuota(async () => {
      expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303);
    });
  }
});

test('denied preference storage keeps an editable custom menu in the current tab', async ({ page, context }) => {
  await context.addInitScript(() => {
    for (const key of ['getItem', 'setItem', 'removeItem']) {
      Object.defineProperty(Storage.prototype, key, { value() { throw new DOMException('Unavailable', 'SecurityError'); } });
    }
  });
  await page.goto('/fixture/');
  const settings = await openSettings(page);
  await settings.locator('#custom-menu-edit').click();
  const editor = page.getByRole('dialog', { name: 'Custom Board List', exact: true });
  await editor.getByLabel('Boards', { exact: true }).fill('demo fixture');
  await editor.getByRole('button', { name: 'Save board list' }).click();
  await expect(editor).toHaveCount(0);
  await settings.getByRole('button', { name: 'Close settings' }).click();
  await expectCustomMenus(page);
  await expect(page.locator('.watcherNotice')).toContainText('Changes stay in this tab');
  await customMenu(page).getByRole('link', { name: 'Edit', exact: true }).click();
  await expect(editor.getByLabel('Boards', { exact: true })).toHaveValue('demo fixture');
  await editor.getByRole('button', { name: 'Cancel' }).click();
});
