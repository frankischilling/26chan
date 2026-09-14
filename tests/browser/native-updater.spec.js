import { test as base, expect } from '@playwright/test';
const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-updater-password';
    const write = form => request.post('/demo/post', { headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password } });
    const response = await write({ resto: '0', sub: 'Owned updater', com: 'Original post' });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/thread\/(\d+)/)[1];
    const remove = no => request.post('/demo/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no, password } });
    try { await use({ id, url: `/demo/thread/${id}`, path: `/_watch/demo/thread/${id}/posts`, remove,
      reply: async com => { const response = await write({ resto: id, com }); expect(response.status()).toBe(303); return response.headers().location.match(/#p(\d+)/)[1]; } }); }
    finally { await remove(id); }
  },
});
const update = page => page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
const status = page => page.locator('.threadNav.desktop .nativeUpdaterStatus').first();
async function initialize(page, owned, settings = {}) {
  await page.addInitScript(settings => {
    localStorage.setItem('4chan-settings', JSON.stringify(settings));
    window.updateEvents = [];
    document.addEventListener('4chanThreadUpdated', event => window.updateEvents.push({
      detail: event.detail, constructor: event.constructor.name, bubbles: event.bubbles, cancelable: event.cancelable,
      menus: document.querySelectorAll('.post .postMenuBtn').length,
    }));
  }, settings);
  await page.goto(owned.url);
}

test('manual update preserves the document, draft and focus, inserts escaped replies, and wires real menus and forms', async ({ page, owned }) => {
  await initialize(page, owned, { keyBinds: true, threadWatcher: true });
  await page.getByRole('button', { name: `Watch thread ${owned.id}`, exact: true }).first().click();
  await page.locator('#togglePostFormLink a').click(); await page.locator('#com').fill('Unsubmitted draft');
  const reply = await owned.reply(`>>${owned.id}\n>green\n[spoiler]fold[/spoiler]\n<script>window.bad=true</script>`);
  const navigation = []; page.on('request', request => { if (request.isNavigationRequest()) navigation.push(request.url()); });
  await page.evaluate(() => { window.keptDocument = true; });
  await update(page);
  await expect(status(page)).toHaveText('1 new post');
  await expect(page.locator('#com')).toHaveValue('Unsubmitted draft');
  expect(await page.evaluate(() => window.keptDocument && !window.bad)).toBe(true);
  expect(navigation).toEqual([]);
  await expect(page.locator(`#m${reply}`)).toContainText('<script>window.bad=true</script>');
  await expect(page.locator(`#m${reply} script`)).toHaveCount(0);
  await expect(page.locator(`#m${reply} .quotelink`)).toHaveAttribute('href', `/demo/post/${owned.id}`);
  // /demo/ has no source spoiler policy; the brackets remain visible.
  await expect(page.locator(`#m${reply}`)).toContainText('[spoiler]fold[/spoiler]');
  await expect(page.locator(`#m${reply} s, #m${reply} .spoiler`)).toHaveCount(0);
  await expect(page.getByRole('button', { name: `Post menu for post ${reply}`, exact: true })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.updateEvents)).toEqual([{ detail: { count: 1 }, constructor: 'Event', bubbles: false, cancelable: false, menus: 2 }]);
  await expect(page.locator('.threadNav.desktop a[data-cmd="update"]').first()).toBeFocused();
  await expect.poll(() => page.evaluate(id => JSON.parse(localStorage.getItem('4chan-watch'))[`${id}-demo`][1], owned.id)).toBe(Number(reply));
  await page.getByRole('button', { name: `Post menu for post ${reply}`, exact: true }).click();
  await page.getByRole('menuitem', { name: 'Report post', exact: true }).click();
  await expect(page.locator(`#report${reply}`)).toBeFocused();
  await page.locator(`#report${reply}`).fill('Owned updater report');
  const response = page.waitForResponse(r => new URL(r.url()).pathname === '/demo/report' && r.request().method() === 'POST');
  await page.locator(`#p${reply}`).getByRole('button', { name: 'Report post', exact: true }).click();
  expect((await response).status()).toBe(200);
});

test('R inserts only new replies once and obeys the editable-field and settings guards', async ({ page, owned }) => {
  await initialize(page, owned, { keyBinds: true });
  const reply = await owned.reply('Reply fetched by R');
  await page.locator('#togglePostFormLink a').click(); await page.locator('#com').focus(); await page.keyboard.press('r');
  await expect(page.locator(`#p${reply}`)).toHaveCount(0);
  await page.locator('h1').click(); await page.keyboard.press('r');
  await expect(status(page)).toHaveText('1 new post');
  await page.waitForTimeout(1100); await update(page);
  await expect(status(page)).toHaveText('No new posts');
  await expect(page.locator(`#p${reply}`)).toHaveCount(1);
  expect(await page.evaluate(() => window.updateEvents.length)).toBe(1);
});

test('a malformed last fragment prevents all insertion and cannot initiate foreign requests', async ({ page, request, owned }) => {
  await initialize(page, owned);
  const first = await owned.reply('First new reply'), second = await owned.reply('Second new reply');
  const snapshot = await (await request.get(owned.path)).json();
  snapshot.posts.at(-1).html = snapshot.posts.at(-1).html.replace('</blockquote>', '<img src="http://127.0.0.1:3003/boards.json" onerror="window.bad=true"></blockquote>');
  const unexpected = []; page.on('request', request => { if (new URL(request.url()).port === '3003') unexpected.push(request.url()); });
  await page.route(`**${owned.path}`, route => route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) }));
  await update(page); await expect(status(page)).toContainText('Connection Error');
  await expect(page.locator(`#p${first}, #p${second}`)).toHaveCount(0);
  expect(unexpected).toEqual([]); expect(await page.evaluate(() => window.bad)).toBeUndefined();
  expect(await page.evaluate(() => window.updateEvents)).toEqual([]);
  await page.unroute(`**${owned.path}`); await page.waitForTimeout(1100); await update(page);
  await expect(status(page)).toHaveText('2 new posts');
});

test('cross-tab disable cancels an outstanding response and re-enabling permits a fresh update', async ({ page, context, request, owned }) => {
  await initialize(page, owned);
  const other = await context.newPage(); await other.goto(owned.url);
  const reply = await owned.reply('Late reply');
  const snapshot = await (await request.get(owned.path)).json();
  let release, intercepted;
  const waiting = new Promise(resolve => { intercepted = resolve; });
  await page.route(`**${owned.path}`, async route => {
    intercepted(); await new Promise(resolve => { release = resolve; });
    await route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) }).catch(() => {});
  });
  await update(page); await waiting;
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await expect(page.locator('.threadNav.desktop .nativeUpdater').first()).toBeHidden();
  release(); await page.unrouteAll({ behavior: 'wait' });
  await expect(page.locator(`#p${reply}`)).toHaveCount(0);
  await other.evaluate(() => localStorage.setItem('4chan-settings', '{}'));
  await expect(page.locator('.threadNav.desktop .nativeUpdater').first()).toBeVisible();
  await page.waitForTimeout(1100); await update(page);
  await expect(status(page)).toHaveText('1 new post');
});

test('404 is terminal while transient failures preserve a usable retry', async ({ page, owned }) => {
  await initialize(page, owned);
  await page.route(`**${owned.path}`, route => route.fulfill({ status: 503, body: 'Unavailable' }));
  await update(page); await expect(status(page)).toContainText('Connection Error');
  await page.unrouteAll(); await page.waitForTimeout(1100);
  await owned.remove(owned.id); await update(page);
  await expect(status(page)).toHaveText('This thread has been pruned or deleted');
  let fetched = 0; page.on('request', request => { if (new URL(request.url()).pathname === owned.path) fetched++; });
  await page.waitForTimeout(1100); await update(page);
  expect(fetched).toBe(0); await expect(page.locator(`#p${owned.id}`)).toBeVisible();
});

test('new replies participate in filters and ordinary reply hiding', async ({ page, owned }) => {
  await initialize(page, owned, { filter: true });
  await page.evaluate(() => localStorage.setItem('4chan-filters', JSON.stringify([{ active: true, pattern: 'Filtered new reply', boards: 'demo', type: 2, hide: true }])));
  const hidden = await owned.reply('Filtered new reply'), visible = await owned.reply('Visible new reply');
  await update(page); await expect(status(page)).toHaveText('2 new posts');
  await expect(page.locator(`#p${hidden}`)).toHaveClass(/post-hidden/);
  await expect(page.getByRole('button', { name: `View filtered post ${hidden}`, exact: true })).toBeVisible();
  await page.getByRole('button', { name: `Post menu for post ${visible}`, exact: true }).click();
  await page.getByRole('menuitem', { name: 'Hide post', exact: true }).click();
  await expect(page.locator(`#pc${visible}`)).toHaveClass(/post-hidden/);
  await expect(page.locator(`#m${visible}`)).toBeHidden();
});

test('closed, reopened and archived snapshots update state and retain the posting draft', async ({ page, request, owned }) => {
  await initialize(page, owned);
  await page.locator('#togglePostFormLink a').click(); await page.locator('#com').fill('Retained across thread states');
  const snapshot = await (await request.get(owned.path)).json();
  await page.route(`**${owned.path}`, route => route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) }));
  snapshot.closed = true; snapshot.sticky = true;
  await update(page); await expect(status(page)).toHaveText('No new posts');
  await expect(page.locator('#com')).toBeDisabled();
  await expect(page.locator(`#pi${owned.id} .nativeThreadState`)).toHaveText(['Sticky', 'Closed']);
  snapshot.closed = false; snapshot.sticky = false;
  await page.waitForTimeout(1100); await update(page); await expect(status(page)).toHaveText('No new posts');
  await expect(page.locator('#com')).toBeEnabled();
  await expect(page.locator('#com')).toHaveValue('Retained across thread states');
  snapshot.archived = true;
  await page.waitForTimeout(1100); await update(page); await expect(status(page)).toHaveText('This thread is archived');
  await expect(page.locator('#com')).toBeDisabled();
  await expect(page.locator(`#t${owned.id}`)).toHaveAttribute('data-archived', 'true');
  expect(await page.evaluate(() => window.updateEvents)).toEqual([]);
});

test('a held watcher lock cannot stall insertion or commit a stale read position after its deadline', async ({ page, context, owned }) => {
  await initialize(page, owned, { threadWatcher: true });
  await page.getByRole('button', { name: `Watch thread ${owned.id}`, exact: true }).first().click();
  await expect(page.locator(`#watch-${owned.id}-demo`)).toBeVisible();
  const other = await context.newPage(); await other.goto(owned.url);
  await other.evaluate(() => {
    window.lockHeld = false;
    void navigator.locks.request('paperboard-thread-watcher', async () => {
      window.lockHeld = true; await new Promise(resolve => { window.releaseUpdaterLock = resolve; });
    });
  });
  await expect.poll(() => other.evaluate(() => window.lockHeld)).toBe(true);
  try {
    const reply = await owned.reply('Inserted while watch storage is busy');
    await update(page);
    await expect(page.locator(`#p${reply}`)).toBeVisible();
    await expect(status(page)).toHaveText('1 new post');
    expect(await page.evaluate(() => window.updateEvents.length)).toBe(1);
    await other.evaluate(() => window.releaseUpdaterLock());
    // Drain all queued lock requests before checking that the expired write
    // cannot clear a newer unread state or advance an old read position.
    await other.evaluate(() => navigator.locks.request('paperboard-thread-watcher', () => {}));
    expect(await page.evaluate(id => JSON.parse(localStorage.getItem('4chan-watch'))[`${id}-demo`][1], owned.id)).toBe(Number(owned.id));
    const next = await owned.reply('Fresh update after lock release');
    await update(page); await expect(status(page)).toHaveText('1 new post');
    await expect.poll(() => page.evaluate(id => JSON.parse(localStorage.getItem('4chan-watch'))[`${id}-demo`][1], owned.id)).toBe(Number(next));
  } finally { await other.evaluate(() => window.releaseUpdaterLock()); }
});
