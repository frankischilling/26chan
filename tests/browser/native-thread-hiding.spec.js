import { test as base, expect } from '@playwright/test';

const key = '4chan-hide-t-demo', purgeKey = '4chan-purge-t-demo', lock = 'paperboard-thread-hiding-demo';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-native-thread-hiding-password';
    const response = await request.post('/demo/post', {
      headers: { Origin: 'http://127.0.0.1:3000' }, maxRedirects: 0,
      form: { resto: '0', sub: 'Owned native thread hiding', com: 'Owned thread hiding OP', password },
    });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/thread\/(\d+)/)[1];
    try {
      const reply = await request.post('/demo/post', {
        headers: { Origin: 'http://127.0.0.1:3000' }, maxRedirects: 0,
        form: { resto: id, com: 'Owned thread hiding reply', password },
      });
      expect(reply.status()).toBe(303);
      await use({ id, reply: reply.headers().location.match(/#p(\d+)/)[1], url: `/demo/thread/${id}` });
    } finally {
      const removed = await request.post('/demo/delete', {
        headers: { Origin: 'http://127.0.0.1:3000' }, maxRedirects: 0, form: { no: id, password },
      });
      expect(removed.status()).toBe(303);
    }
  },
});
const stored = page => page.evaluate(key => localStorage.getItem(key), key);
async function menu(page, id, action) {
  await page.getByRole('button', { name: `Post menu for post ${id}`, exact: true }).click();
  await page.getByRole('menuitem', { name: action, exact: true }).click();
}
async function settings(page) {
  await page.getByRole('link', { name: 'Settings', exact: true }).click();
  if (!(await page.locator('#setting-threadHiding').isVisible())) {
    await page.getByRole('button', { name: 'Filters & Post Hiding', exact: true }).click();
  }
}

test('desktop native icons hide a persisted thread and its replies, and restore after navigation', async ({ page, owned }) => {
  await page.goto('/demo/');
  const control = page.locator(`#sa${owned.id}`);
  await expect(control).toHaveAccessibleName(`Hide thread ${owned.id}`);
  await expect(control.locator('img')).toHaveAttribute('src', /post_expand_minus\.png$/);
  await control.click();
  await expect(page.locator(`#m${owned.id}`)).toBeHidden();
  await expect(page.locator(`#pc${owned.reply}`)).toBeHidden();
  await expect(page.locator(`#pi${owned.id}`)).toBeVisible();
  await expect(control.locator('img')).toHaveAttribute('src', /post_expand_plus\.png$/);
  await expect.poll(async () => Object.keys(JSON.parse(await stored(page)))).toContain(owned.id);
  await page.reload();
  await expect(page.locator(`#m${owned.id}`)).toBeHidden();
  await menu(page, owned.id, 'Unhide thread');
  await expect(page.locator(`#m${owned.id}`)).toBeVisible();
  await expect.poll(() => stored(page)).toBe(null);
  await page.goto(owned.url);
  await expect(page.locator('.nativeThreadToggle')).toHaveCount(0);
  await page.getByRole('button', { name: `Post menu for post ${owned.id}`, exact: true }).click();
  await expect(page.locator('#post-menu [data-cmd="hide"]')).toHaveCount(0);
});

test('mobile hides use the external Show Hidden Thread control and follow viewport changes', async ({ page, owned }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/demo/');
  await menu(page, owned.id, 'Hide thread');
  await expect(page.locator(`#t${owned.id}`)).toBeHidden();
  const restore = page.locator(`#sa${owned.id}`);
  await expect(restore).toHaveText('Show Hidden Thread');
  await expect(restore).toBeFocused();
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(page.locator(`#t${owned.id}`)).toBeVisible();
  await expect(page.locator(`#m${owned.id}`)).toBeHidden();
  await expect(restore.locator('img')).toHaveAttribute('src', /post_expand_plus\.png$/);
  await page.setViewportSize({ width: 390, height: 844 });
  await restore.click();
  await expect(page.locator(`#m${owned.id}`)).toBeVisible();
  await expect(page.getByRole('button', { name: `Post menu for post ${owned.id}`, exact: true })).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('cross-tab storage updates synchronize an open OP menu without changing reply history', async ({ page, context, owned }) => {
  await page.goto('/demo/');
  const other = await context.newPage(); await other.goto('/demo/');
  await other.getByRole('button', { name: `Post menu for post ${owned.id}`, exact: true }).click();
  await page.locator(`#sa${owned.id}`).click();
  await expect(other.getByRole('menuitem', { name: 'Unhide thread', exact: true })).toBeVisible();
  await other.getByRole('menuitem', { name: 'Unhide thread', exact: true }).click();
  await expect(page.locator(`#m${owned.id}`)).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem('4chan-hide-r-demo'))).toBe(null);
});

test('hide-stub settings and native Clear History recover a completely hidden thread on settings navigation', async ({ page, owned }) => {
  await page.goto('/demo/');
  await settings(page);
  await expect(page.locator('#setting-threadHiding')).toBeChecked();
  await page.locator('#setting-hideStubs').check();
  await Promise.all([page.waitForEvent('load'), page.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
  await page.locator(`#sa${owned.id}`).click();
  await expect(page.locator(`#t${owned.id}`)).toBeHidden();
  await settings(page);
  page.once('dialog', dialog => {
    expect(dialog.message()).toBe('This will unhide 1 thread on /demo/');
    return dialog.accept();
  });
  await page.getByRole('link', { name: 'Clear History', exact: true }).click();
  await expect.poll(() => stored(page)).toBe(null);
  await Promise.all([page.waitForEvent('load'), page.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
  await expect(page.locator(`#m${owned.id}`)).toBeVisible();
});

test('a successful real live-list cleanup removes absent IDs but not live hidden threads', async ({ page, request, owned }) => {
  const absent = '9223372036854775807';
  expect((await request.get(`/demo/thread/${absent}.json`)).status()).toBe(404);
  await page.goto(owned.url);
  await page.evaluate(({ key, purgeKey, id, absent }) => {
    localStorage.setItem(key, JSON.stringify({ [id]: 1, [absent]: 1 }));
    localStorage.removeItem(purgeKey);
  }, { key, purgeKey, id: owned.id, absent });
  await page.goto('/demo/');
  await expect.poll(async () => JSON.parse(await stored(page))).toEqual({ [owned.id]: 1 });
  await expect.poll(() => page.evaluate(key => Number(localStorage.getItem(key)), purgeKey)).toBeGreaterThan(1);
  await expect(page.locator(`#m${owned.id}`)).toBeHidden();
});

test('invalid saved hides remain untouched and lockless edits stay in the current tab', async ({ page, owned }) => {
  await page.goto(owned.url);
  await page.evaluate(key => localStorage.setItem(key, '{'), key);
  await page.goto('/demo/');
  await expect(page.locator('.nativeThreadNotice')).toContainText('invalid');
  await expect(page.locator(`#m${owned.id}`)).toBeVisible();
  expect(await stored(page)).toBe('{');
  await page.evaluate(key => localStorage.removeItem(key), key);
  await page.addInitScript(() => Object.defineProperty(navigator, 'locks', { value: undefined }));
  await page.reload();
  await page.locator(`#sa${owned.id}`).click();
  await expect(page.locator(`#m${owned.id}`)).toBeHidden();
  await expect(page.locator('.nativeThreadNotice')).toContainText('only in this tab');
  expect(await stored(page)).toBe(null);
});

test('disabling thread hiding abandons a queued edit behind a real browser lock', async ({ page, context, owned }) => {
  await page.goto('/demo/');
  const other = await context.newPage(); await other.goto(owned.url);
  await other.evaluate(lock => {
    window.threadLockTask = navigator.locks.request(lock, async () => {
      window.threadLockHeld = true;
      await new Promise(resolve => { window.releaseThreadLock = resolve; });
    });
  }, lock);
  await other.waitForFunction(() => window.threadLockHeld);
  await page.locator(`#sa${owned.id}`).click();
  await expect.poll(() => page.evaluate(async lock => (await navigator.locks.query()).pending.some(row => row.name === lock), lock)).toBe(true);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadHiding: false })));
  await expect(page.locator(`#sa${owned.id}`)).toBeHidden();
  await other.evaluate(async () => { window.releaseThreadLock(); await window.threadLockTask; });
  await page.evaluate(lock => navigator.locks.request(lock, () => {}), lock);
  expect(await stored(page)).toBe(null);
  await expect(page.locator(`#m${owned.id}`)).toBeVisible();
});

for (const hide of [true, false]) {
  test(`${hide ? 'Hide' : 'Highlight'} filters preserve native manual thread-hide restoration precedence`, async ({ page, owned }) => {
    await page.goto(owned.url);
    await page.evaluate(({ key, purgeKey, id, hide }) => {
      localStorage.setItem(key, JSON.stringify({ [id]: 1 }));
      localStorage.setItem(purgeKey, String(Date.now()));
      localStorage.setItem('4chan-settings', JSON.stringify({ filter: true }));
      localStorage.setItem('4chan-filters', JSON.stringify([
        { active: true, type: 5, pattern: 'Owned native thread hiding', boards: 'demo', hide, auto: false },
      ]));
    }, { key, purgeKey, id: owned.id, hide });
    await page.goto('/demo/');
    await expect(page.locator(`#t${owned.id}`)).toHaveClass(/post-hidden/);
    if (hide) {
      await expect(page.locator(`#sa${owned.id}`)).toBeHidden();
      expect(JSON.parse(await stored(page))[owned.id]).toBe(1);
    } else {
      await expect(page.locator(`#sa${owned.id}`)).toBeVisible();
      await expect.poll(async () => JSON.parse(await stored(page))[owned.id]).toBeGreaterThan(1);
    }
  });
}

for (const failure of [true, false]) {
  test(`${failure ? 'failed' : 'stale successful'} live-list cleanup cannot erase current hidden history`, async ({ page, context, owned }) => {
    const absent = '9223372036854775807';
    await page.goto(owned.url);
    await page.evaluate(({ key, purgeKey, absent }) => {
      localStorage.setItem(key, JSON.stringify({ [absent]: 1 })); localStorage.removeItem(purgeKey);
    }, { key, purgeKey, absent });
    const other = await context.newPage(); await other.goto(owned.url);
    let captured, release;
    const capturedRequest = new Promise(resolve => { captured = resolve; });
    const gate = new Promise(resolve => { release = resolve; });
    await page.route('**/_watch/demo/catalog.json', async route => {
      const response = failure ? null : await route.fetch();
      captured(); await gate;
      if (failure) await route.fulfill({ status: 503, contentType: 'application/json', body: '{}' });
      else await route.fulfill({ response });
    });
    try {
      await page.goto('/demo/'); await capturedRequest;
      if (!failure) {
        await page.locator(`#sa${owned.id}`).click();
        await expect.poll(async () => Object.hasOwn(JSON.parse(await stored(page)), owned.id)).toBe(true);
      }
      await other.evaluate(lock => {
        window.cleanupLockTask = navigator.locks.request(lock, async () => {
          window.cleanupLockHeld = true;
          await new Promise(resolve => { window.releaseCleanupLock = resolve; });
        });
      }, lock);
      await other.waitForFunction(() => window.cleanupLockHeld);
      release();
      await expect.poll(() => page.evaluate(async lock => (await navigator.locks.query()).pending.some(row => row.name === lock), lock)).toBe(true);
      await other.evaluate(async () => { window.releaseCleanupLock(); await window.cleanupLockTask; });
      await page.evaluate(lock => navigator.locks.request(lock, () => {}), lock);
      const saved = JSON.parse(await stored(page));
      expect(saved[absent]).toBe(1);
      if (!failure) expect(saved[owned.id]).toBeGreaterThan(1);
      expect(await page.evaluate(key => localStorage.getItem(key), purgeKey)).toBe(null);
    } finally {
      release();
      await other.evaluate(() => window.releaseCleanupLock?.());
    }
  });
}
