import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';

async function openSettings(page) {
  const custom = page.locator('.customBoardList');
  if (await custom.count()) await custom.getByRole('link', { name: 'Settings', exact: true }).click();
  else await page.locator('#settingsWindowLink:visible, #settingsWindowLinkMobile:visible').click();
  const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
  await dialog.locator('#settings-expand-all').click();
  return dialog;
}

test('custom navigation persists, synchronizes across tabs and remains editable on mobile', async ({ page, context }) => {
  await page.goto('/test/');
  let settings = await openSettings(page);
  await settings.locator('#custom-menu-edit').click();
  const editor = page.getByRole('dialog', { name: 'Custom Board List', exact: true });
  await editor.getByLabel('Boards', { exact: true }).fill('demo test');
  await editor.getByRole('button', { name: 'Save board list', exact: true }).click();
  await expect(editor).toHaveCount(0);
  await settings.getByRole('button', { name: 'Close settings' }).click();
  await expect(page.getByRole('navigation', { name: 'Custom board navigation' })).toBeVisible();
  await expect(page.locator('.customBoardList a').first()).toHaveAttribute('href', '/demo/');
  await page.reload();
  await expect(page.locator('.customBoardList')).toBeVisible();
  const other = await context.newPage();
  try {
    await other.goto('/test/');
    await expect(other.locator('.customBoardList')).toBeVisible();
    await page.locator('.customBoardList').getByRole('link', { name: 'Edit', exact: true }).click();
    await editor.getByLabel('Boards', { exact: true }).fill('test demo');
    await editor.getByRole('button', { name: 'Save board list' }).click();
    await expect(other.locator('.customBoardList a').first()).toHaveAttribute('href', '/test/');
    await page.getByRole('link', { name: 'Show all boards' }).click();
    await expect(page.locator('.customBoardList')).toHaveCount(0);
    await expect(page.getByRole('navigation', { name: 'Board navigation', exact: true })).toBeVisible();
    await expect(other.locator('.customBoardList')).toBeVisible();
    await page.setViewportSize({ width: 390, height: 844 });
    await page.reload();
    await expect(page.locator('.customBoardList')).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    settings = await openSettings(page);
    await settings.getByLabel('Custom board list', { exact: true }).uncheck();
    await Promise.all([page.waitForEvent('load'), settings.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
    await expect(page.locator('.customBoardList')).toHaveCount(0);
    await expect(other.locator('.customBoardList')).toHaveCount(0);
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).customMenuList)).toBe('test demo');
  } finally { await other.close(); }
});

test('local time uses the real timestamp while settings and script-free pages preserve server output', async ({ browser, request }) => {
  const password = 'owned-display-password';
  const created = await request.post('/test/post', {
    headers: { Origin: origin }, maxRedirects: 0,
    form: { resto: '0', com: 'Owned display date.', password },
  });
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
    const settings = await openSettings(page);
    await expect(settings.getByLabel('Convert dates to local time', { exact: true })).toBeChecked();
    await settings.getByLabel('Convert dates to local time', { exact: true }).uncheck();
    await Promise.all([page.waitForEvent('load'), settings.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
    await expect(clock).toHaveText(server);
    await expect(clock).not.toHaveAttribute('title');
    const response = await request.get(`/test/thread/${id}.json`);
    const data = await response.json();
    expect(typeof data.posts[0].time).toBe('number');
    expect(data.posts[0].now).toBe(server);
  } finally {
    await context.close(); await plain.close();
    expect((await request.post('/test/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303);
  }
});

test('denied preference storage keeps an editable custom menu in the current tab', async ({ page, context }) => {
  await context.addInitScript(() => {
    for (const key of ['getItem', 'setItem', 'removeItem']) {
      Object.defineProperty(Storage.prototype, key, { value() { throw new DOMException('Unavailable', 'SecurityError'); } });
    }
  });
  await page.goto('/test/');
  const settings = await openSettings(page);
  await settings.locator('#custom-menu-edit').click();
  const editor = page.getByRole('dialog', { name: 'Custom Board List', exact: true });
  await editor.getByLabel('Boards', { exact: true }).fill('demo test');
  await editor.getByRole('button', { name: 'Save board list' }).click();
  await expect(editor).toHaveCount(0);
  await settings.getByRole('button', { name: 'Close settings' }).click();
  await expect(page.locator('.customBoardList')).toBeVisible();
  await expect(page.locator('.watcherNotice')).toContainText('Changes stay in this tab');
  await page.locator('.customBoardList').getByRole('link', { name: 'Edit', exact: true }).click();
  await expect(editor.getByLabel('Boards', { exact: true })).toHaveValue('demo test');
  await editor.getByRole('button', { name: 'Cancel' }).click();
});
