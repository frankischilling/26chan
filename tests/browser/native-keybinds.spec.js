import { test as base, expect } from '@playwright/test';

const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-native-keybind-password', ids = [];
    const create = async () => {
      const response = await request.post('/demo/post', {
        headers: { Origin: 'http://127.0.0.1:3000' }, maxRedirects: 0,
        form: { resto: '0', sub: 'Owned keyboard shortcut', com: 'Owned keyboard shortcut text', password },
      });
      expect(response.status()).toBe(303);
      const id = response.headers().location.match(/thread\/(\d+)/)[1]; ids.push(id); return id;
    };
    try {
      const id = await create(); await use({ id, url: `/demo/thread/${id}`, create });
    } finally {
      for (const id of ids.reverse()) {
        const response = await request.post('/demo/delete', {
          headers: { Origin: 'http://127.0.0.1:3000' }, maxRedirects: 0, form: { no: id, password },
        });
        expect(response.status()).toBe(303);
      }
    }
  },
});
const watch = (page, id) => page.locator(`#watch-${id}-demo`);
async function press(page, key) { await page.locator('h1').click(); await page.keyboard.press(key); }
async function enable(page, url, extra = {}) {
  await page.goto(url);
  await page.evaluate(extra => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true, threadWatcher: true, ...extra })), extra);
  await page.reload();
}
async function openNavigation(page) {
  await page.getByRole('link', { name: 'Settings', exact: true }).click();
  if (!(await page.locator('#setting-keyBinds').isVisible())) {
    await page.getByRole('button', { name: 'Navigation', exact: true }).click();
  }
}

test('Settings opt-in persists and W uses the real shared watcher state only on thread pages', async ({ page, context, owned }) => {
  await page.goto(owned.url); await press(page, 'w'); await expect(watch(page, owned.id)).toHaveCount(0);
  await openNavigation(page);
  await expect(page.locator('#setting-keyBinds')).not.toBeChecked();
  await page.locator('#setting-keyBinds').check();
  if (!(await page.locator('#setting-threadWatcher').isVisible())) {
    await page.getByRole('button', { name: 'Monitoring', exact: true }).click();
  }
  await page.locator('#setting-threadWatcher').check();
  await Promise.all([page.waitForEvent('load'), page.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
  const other = await context.newPage(); await other.goto(owned.url);
  await press(page, 'w');
  await expect(watch(page, owned.id)).toBeVisible(); await expect(watch(other, owned.id)).toBeVisible();
  await press(page, 'w');
  await expect(watch(page, owned.id)).toHaveCount(0); await expect(watch(other, owned.id)).toHaveCount(0);
  await page.goto('/demo/'); await press(page, 'w'); await expect(watch(page, owned.id)).toHaveCount(0);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).keyBinds)).toBe(true);
});

test('editing fields, modifiers and disabled features do not accidentally watch a thread', async ({ page, context, owned }) => {
  await enable(page, owned.url);
  await page.locator('#togglePostFormLink a').click();
  for (const selector of ['input[name="name"]', 'textarea[name="com"]']) {
    const input = page.locator(`form[action="/demo/imgboard.php"] ${selector}`).first();
    await input.fill(''); await input.press('w'); await expect(input).toHaveValue('w');
    await expect(watch(page, owned.id)).toHaveCount(0);
  }
  await press(page, 'Shift+W'); await expect(watch(page, owned.id)).toHaveCount(0);
  const other = await context.newPage(); await other.goto(owned.url);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true, threadWatcher: false })));
  await expect(page.locator('#threadWatcher')).toBeHidden();
  await press(page, 'w'); await expect(watch(page, owned.id)).toHaveCount(0);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true, threadWatcher: true, disableAll: true })));
  await expect(page.locator('[data-post-menu]:visible')).toHaveCount(0);
  await press(page, 'w'); await expect(watch(page, owned.id)).toHaveCount(0);
});

test('F opens the existing filter editor with selection from a persisted post', async ({ page, owned }) => {
  await enable(page, owned.url, { filter: true });
  await page.locator('h1').click();
  await page.locator(`#m${owned.id}`).evaluate(element => {
    const range = document.createRange(); range.selectNodeContents(element);
    const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
  });
  await page.keyboard.press('f');
  await expect(page.getByRole('dialog', { name: 'Filters & Highlights', exact: true })).toBeVisible();
  expect(await page.locator('#filtersMenu').evaluate(dialog => [...dialog.querySelectorAll('input,textarea')].some(field => field.value === 'Owned keyboard shortcut text'))).toBe(true);
  await page.keyboard.press('Escape');
});

test('B/N follow actual persisted pagination and I/C navigate without submitting the post form', async ({ page, owned }) => {
  for (let i = 0; i < 11; i++) await owned.create();
  await enable(page, owned.url);
  const posts = [];
  page.on('request', request => { if (request.method() === 'POST') posts.push(request.url()); });
  await Promise.all([page.waitForURL('**/demo/'), press(page, 'i')]);
  const next = await page.locator('.pages a[rel="next"]').getAttribute('href');
  await Promise.all([page.waitForURL(new URL(next, page.url()).href), press(page, 'n')]);
  const previous = await page.locator('.pages a[rel="prev"]').getAttribute('href');
  await Promise.all([page.waitForURL(new URL(previous, page.url()).href), press(page, 'b')]);
  await Promise.all([page.waitForURL('**/demo/catalog'), press(page, 'c')]);
  expect(posts).toEqual([]);
});

test('shortcut help is keyboard-dismissable and advertises connected actions', async ({ page, owned }) => {
  await enable(page, owned.url);
  await openNavigation(page);
  await page.getByRole('link', { name: 'Show', exact: true }).click();
  const help = page.getByRole('dialog', { name: 'Keyboard Shortcuts', exact: true });
  await expect(help).toBeVisible(); await expect(help).toContainText('Watch/Unwatch thread');
  await expect(help).toContainText('Open Quick Reply');
  await expect(help).toContainText('Toggle auto-updater');
  await page.keyboard.press('Escape'); await expect(help).toHaveCount(0);
  await expect(page.getByRole('link', { name: 'Show', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  const requests = [];
  page.on('request', request => { if (request.isNavigationRequest() || request.method() === 'POST') requests.push(request.url()); });
  for (const key of ['a', 'q', 'r']) await press(page, key);
  expect(requests).toEqual([]);
  await expect(page).toHaveURL(new RegExp(`${owned.url}$`));
});
