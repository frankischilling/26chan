import { test as base, expect } from '@playwright/test';

const key = '4chan-hide-r-demo';
const lock = 'paperboard-reply-hiding-demo';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-reply-hiding-password';
    const write = form => request.post('/demo/post', { headers: { Origin: 'http://127.0.0.1:3000' },
      form: { ...form, password }, maxRedirects: 0 });
    const response = await write({ resto: '0', sub: 'Owned reply hiding', com: 'Owned visible OP' });
    expect(response.status()).toBe(303);
    const thread = response.headers().location.match(/thread\/(\d+)/)[1];
    try {
      const replies = [];
      for (const label of ['first', 'second']) {
        const reply = await write({ resto: thread, com: `Owned hidden ${label}` });
        expect(reply.status()).toBe(303);
        replies.push(reply.headers().location.match(/#p(\d+)/)[1]);
      }
      await use({ thread, replies, url: `/demo/thread/${thread}` });
    } finally {
      const removed = await request.post('/demo/delete', { headers: { Origin: 'http://127.0.0.1:3000' },
        form: { no: thread, password }, maxRedirects: 0 });
      expect(removed.status()).toBe(303);
    }
  },
});
const stored = page => page.evaluate(key => localStorage.getItem(key), key);
async function menu(page, id, action) {
  await page.getByRole('button', { name: `Post menu for post ${id}`, exact: true }).click();
  await page.getByRole('menuitem', { name: action, exact: true }).click();
}
async function hold(page) {
  await page.evaluate(lock => {
    window.replyLockHeld = false;
    window.replyLockTask = navigator.locks.request(lock, async () => {
      window.replyLockHeld = true;
      await new Promise(resolve => { window.releaseReplyLock = resolve; });
    });
  }, lock);
  await page.waitForFunction(() => window.replyLockHeld);
}

test('native reply menus persist board-scoped hides and synchronize an already-open menu', async ({ page, context, owned }) => {
  const [id] = owned.replies;
  await page.goto(owned.url);
  const other = await context.newPage(); await other.goto(owned.url);
  await other.getByRole('button', { name: `Post menu for post ${id}`, exact: true }).click();
  await expect(other.getByRole('menuitem', { name: 'Hide post', exact: true })).toBeVisible();
  await menu(page, id, 'Hide post');
  await expect(page.locator(`#m${id}`)).toBeHidden();
  await expect(page.locator(`#sa${id}`)).toHaveAttribute('data-hidden', id);
  await expect(other.getByRole('menuitem', { name: 'Unhide post', exact: true })).toBeVisible();
  await expect(page.locator(`#p${owned.thread}`)).toBeVisible();
  await expect.poll(async () => Object.keys(JSON.parse(await stored(page)))).toEqual([id]);
  await page.goto('/demo/'); await expect(page.locator(`#m${id}`)).toBeHidden();
  await page.goto(owned.url); await expect(page.locator(`#m${id}`)).toBeHidden();
  await other.getByRole('menuitem', { name: 'Unhide post', exact: true }).click();
  await expect(page.locator(`#m${id}`)).toBeVisible();
  await expect.poll(() => stored(page)).toBe(null);
  await page.getByRole('button', { name: `Post menu for post ${owned.thread}`, exact: true }).click();
  await expect(page.locator('[data-cmd="hide-r"]')).toHaveCount(0);
});

test('queued edits merge different replies and disabling cancels a waiting hide', async ({ page, context, owned }) => {
  const [first, second] = owned.replies;
  await page.goto(owned.url);
  const other = await context.newPage(); await other.goto(owned.url);
  await hold(other);
  await menu(page, first, 'Hide post'); await menu(other, second, 'Hide post');
  await other.evaluate(() => window.releaseReplyLock());
  await expect.poll(async () => Object.keys(JSON.parse(await stored(page)) ?? {}).sort()).toEqual([first, second].sort());
  await expect(page.locator(`#m${first}`)).toBeHidden(); await expect(page.locator(`#m${second}`)).toBeHidden();
  await menu(page, first, 'Unhide post'); await expect(page.locator(`#m${first}`)).toBeVisible();
  await hold(other); await menu(page, first, 'Hide post');
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await expect(page.locator('[data-post-menu]:visible')).toHaveCount(0);
  await other.evaluate(() => window.releaseReplyLock());
  await expect.poll(async () => Object.keys(JSON.parse(await stored(page)))).toEqual([second]);
  await expect(page.locator(`#m${second}`)).toBeVisible();
  await other.evaluate(() => localStorage.setItem('4chan-settings', '{}'));
  await expect(page.locator(`#m${second}`)).toBeHidden();
});

test('manual hides remain independent of filter View and mobile menus remain usable', async ({ page, owned }) => {
  const [id] = owned.replies;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(owned.url);
  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ filter: true }));
    localStorage.setItem('4chan-filters', JSON.stringify([{ active: true, type: 2, pattern: 'Owned hidden first', boards: '', hide: true, auto: false }]));
  });
  await page.reload();
  const view = page.getByRole('button', { name: `View filtered post ${id}`, exact: true });
  await expect(view).toBeVisible();
  await menu(page, id, 'Hide post');
  await expect(page.locator(`#pc${id}`)).toHaveClass(/post-hidden/);
  await view.click(); await expect(page.locator(`#m${id}`)).toBeHidden();
  await menu(page, id, 'Unhide post'); await expect(page.locator(`#m${id}`)).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('reload gives Hide filters precedence while highlight matches still restore manual hides', async ({ page, owned }) => {
  const [id, unmatched] = owned.replies;
  await page.goto(owned.url);
  for (const rule of [{ hide: true }, { hide: false, color: '#ff0000' }, { hide: false }]) {
    const old = Date.now() - 3600000;
    await page.evaluate(({ key, id, unmatched, old, rule }) => {
      localStorage.setItem(key, JSON.stringify({ [id]: old, [unmatched]: old }));
      localStorage.setItem('4chan-settings', JSON.stringify({ filter: true }));
      localStorage.setItem('4chan-filters', JSON.stringify([{ active: true, type: 2, pattern: 'Owned hidden first', boards: '', auto: false, ...rule }]));
    }, { key, id, unmatched, old, rule });
    await page.reload();
    await expect(page.locator('.nativeFilterNotice')).toHaveText('');
    await expect(page.locator(`#pc${unmatched}`)).toHaveClass(/post-hidden/);
    await expect.poll(async () => JSON.parse(await stored(page))[unmatched]).toBeGreaterThan(old);
    if (rule.hide) {
      await expect(page.locator(`#pc${id}`)).not.toHaveClass(/post-hidden/);
      expect(JSON.parse(await stored(page))[id]).toBe(old);
      await page.getByRole('button', { name: `View filtered post ${id}`, exact: true }).click();
      await expect(page.locator(`#m${id}`)).toBeVisible();
      await menu(page, id, 'Hide post');
      await expect(page.locator(`#m${id}`)).toBeHidden();
      await menu(page, id, 'Unhide post');
      await expect(page.locator(`#m${id}`)).toBeVisible();
    } else {
      await expect(page.locator(`#p${id}`)).toHaveClass(/filter-hl/);
      await expect(page.locator(`#pc${id}`)).toHaveClass(/post-hidden/);
      await expect.poll(async () => JSON.parse(await stored(page))[id]).toBeGreaterThan(old);
    }
  }
  await page.evaluate(({ key, id }) => {
    localStorage.setItem(key, JSON.stringify({ [id]: Date.now() - 604800001 }));
    localStorage.setItem('4chan-filters', JSON.stringify([{ active: true, type: 2, pattern: 'Owned hidden first', boards: '', auto: false, hide: true }]));
  }, { key, id });
  await page.reload();
  await expect(page.getByRole('button', { name: `View filtered post ${id}`, exact: true })).toBeVisible();
  await expect.poll(() => stored(page)).toBe(null);
  await expect(page.locator(`#pc${id}`)).not.toHaveClass(/post-hidden/);
});

test('malformed storage stays untouched and visited hidden replies renew before expiry pruning', async ({ page, owned }) => {
  const [id] = owned.replies;
  await page.goto(owned.url);
  await page.evaluate(key => localStorage.setItem(key, '{"__proto__":1}'), key); await page.reload();
  await expect(page.locator('.nativeReplyNotice')).toContainText('invalid');
  await menu(page, id, 'Hide post'); await expect(page.locator(`#m${id}`)).toBeVisible();
  expect(await stored(page)).toBe('{"__proto__":1}');
  const old = Date.now() - 604800001;
  await page.evaluate(({ key, id, old, op }) => localStorage.setItem(key, JSON.stringify({ [id]: old, '9223372036854775807': old, [op]: Date.now() })), { key, id, old, op: owned.thread });
  await page.reload();
  await expect(page.locator(`#m${id}`)).toBeHidden();
  await expect(page.locator(`#p${owned.thread}`)).toBeVisible();
  await expect.poll(async () => JSON.parse(await stored(page))[id]).toBeGreaterThan(old);
  expect(JSON.parse(await stored(page))['9223372036854775807']).toBeUndefined();
});

for (const failure of ['writes', 'storage', 'locks']) {
  test(`reply hiding remains reversible in this tab with unavailable ${failure}`, async ({ page, context, owned }) => {
    await context.addInitScript(failure => {
      if (failure === 'locks') Object.defineProperty(navigator, 'locks', { value: undefined });
      else for (const method of failure === 'writes' ? ['setItem', 'removeItem'] : ['getItem', 'setItem', 'removeItem']) {
        const original = Storage.prototype[method];
        Storage.prototype[method] = function (key, ...args) {
          if (key.startsWith('4chan-hide-r-')) throw new Error('Unavailable');
          return original.call(this, key, ...args);
        };
      }
    }, failure);
    const [id] = owned.replies;
    await page.goto(owned.url); await menu(page, id, 'Hide post');
    await expect(page.locator(`#m${id}`)).toBeHidden();
    await expect(page.locator('.nativeReplyNotice')).toContainText('only in this tab');
    await menu(page, id, 'Unhide post'); await expect(page.locator(`#m${id}`)).toBeVisible();
  });
}

test('no-JavaScript pages keep replies visible without inert hide controls', async ({ browser, owned }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  try {
    const page = await context.newPage(); await page.goto(`http://127.0.0.1:3000${owned.url}`);
    await expect(page.locator(`#m${owned.replies[0]}`)).toBeVisible();
    await expect(page.locator('[data-cmd="hide-r"], [data-post-menu]')).toHaveCount(0);
  } finally { await context.close(); }
});
