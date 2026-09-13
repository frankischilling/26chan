import { test as base, expect } from '@playwright/test';
import { saveWatcherSettings } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  createThread: async ({ request }, use) => {
    const created = [];
    await use(async (board, label) => {
      const response = await request.post(`/${board}/post`, { headers: { Origin: origin },
        form: { resto: '0', sub: label, com: 'Synthetic watcher fixture', password: 'watcher-test-password' }, maxRedirects: 0 });
      expect(response.status()).toBe(303);
      const id = response.headers().location.match(/thread\/(\d+)/)[1];
      created.push({ board, id });
      return id;
    });
    // Delete only IDs created by this test, through the ordinary password gate.
    // Teardown also runs after an assertion failure; other threads are untouched.
    for (const { board, id } of created) {
      const deleted = await request.post(`/${board}/delete`, { headers: { Origin: origin },
        form: { no: id, password: 'watcher-test-password' }, maxRedirects: 0 });
      expect(deleted.status()).toBe(303);
      expect((await request.get(`/${board}/thread/${id}.json`)).status()).toBe(404);
    }
  },
});
async function enable(page, path, options) {
  await page.goto(path);
  await saveWatcherSettings(page, { threadWatcher: true }, options);
  await expect(page.locator('#threadWatcher')).toBeVisible();
}

test('owned thread API refresh, cross-tab watch state and read acknowledgement work on two boards', async ({ page, context, request, createThread }) => {
  const a = await createThread('demo', 'Watch a paper model');
  const b = await createThread('test', 'Watch another board');
  await enable(page, '/demo/catalog?q=');
  await page.getByRole('button', { name: `Watch thread ${a}`, exact: true }).click();
  await expect(page.locator(`#watch-${a}-demo`)).toContainText('Watch a paper model');
  const other = await context.newPage();
  await other.goto('/test/catalog?q=');
  await expect(other.locator('#threadWatcher')).toBeVisible();
  await other.getByRole('button', { name: `Watch thread ${b}`, exact: true }).click();
  await expect(page.locator(`#watch-${b}-test`)).toBeVisible();
  const reply = await request.post('/demo/post', { headers: { Origin: origin },
    form: { resto: a, com: 'A new reply for the watcher', password: 'watcher-test-password' }, maxRedirects: 0 });
  expect(reply.status()).toBe(303);
  const id = reply.headers().location.match(/#p(\d+)/)[1];
  const fetched = page.waitForResponse(response => response.url().endsWith(`/_watch/demo/thread/${a}.json`));
  await page.locator('#twPrune').click();
  expect((await fetched).status()).toBe(200);
  await expect(page.locator(`#watch-${a}-demo`)).toContainText('(1)');
  await expect(other.locator(`#watch-${a}-demo`)).toContainText('(1)');
  await other.goto(`/demo/thread/${a}#lr${a}`);
  await expect(other.locator(`#watch-${a}-demo a`)).toHaveText('/demo/ - Watch a paper model');
  await expect(other.locator(`#watch-${a}-demo a`)).not.toHaveClass(/hasNewReplies/);
  await expect(other.locator(`#p${id}`)).toHaveClass(/watcherReadTarget/);
  expect(new URL(other.url()).hash).toBe('');
  await expect(page.locator(`#watch-${a}-demo a`)).toHaveAttribute('href', `/demo/thread/${a}#p${id}`);
  await other.locator(`#p${a} .watcherLastRead`).click();
  await expect(page.locator(`#watch-${a}-demo a`)).toHaveAttribute('href', `/demo/thread/${a}#p${a}`);
  await other.getByRole('button', { name: `Unwatch /demo/ thread ${a}`, exact: true }).click();
  await expect(page.locator(`#watch-${a}-demo`)).toHaveCount(0);
});

test('thread navigation watch controls stay synchronized across both placements in all six themes', async ({ page, context, createThread }) => {
  const id = await createThread('demo', 'Navigation watcher fixture');
  await context.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
    localStorage.setItem('4chan-watch', '{}');
    localStorage.setItem('4chan-tw-timestamp', String(Date.now()));
  });
  const families = { yotsuba: 'futaba', 'yotsuba-b': 'burichan', futaba: 'futaba',
    burichan: 'burichan', tomorrow: 'tomorrow', photon: 'photon' };
  for (const [theme, family] of Object.entries(families)) {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: origin, httpOnly: true, sameSite: 'Lax' }]);
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(`/demo/thread/${id}`);
      await expect(page.locator('.threadNav')).toHaveCount(4);
      await expect(page.locator('.threadNav:visible')).toHaveCount(2);
      await expect(page.locator('.postInfo .wbtn')).toHaveCount(0);
      const controls = page.locator('.threadNav .wbtn');
      await expect(controls).toHaveCount(4);
      const visible = page.getByRole('button', { name: `Watch thread ${id}`, exact: true });
      await expect(visible).toHaveCount(2);
      await visible.first().press('Space');
      for (let index = 0; index < 4; index++) {
        await expect(controls.nth(index)).toHaveAttribute('aria-pressed', 'true');
        await expect(controls.nth(index)).toHaveAttribute('data-active', '1');
        await expect(controls.nth(index).locator('img')).toHaveAttribute('src', `/static/watcher/${family}/watch_thread_on.png`);
      }
      expect(await page.locator('.threadNav.desktop').evaluateAll(navs => navs.every(nav => nav.firstElementChild.classList.contains('watcherNavControl')))).toBe(true);
      expect(await page.locator('.threadNav.mobile').evaluateAll(navs => navs.every(nav => nav.lastElementChild.classList.contains('watcherNavControl')))).toBe(true);
      if (width === 390) {
        const wrapper = page.locator('.threadNav.mobile .watcherNavControl').first();
        await expect(wrapper).toHaveCSS('padding', '6px 10px 5px');
        await expect(wrapper).toHaveCSS('background-image', `url("${origin}/static/watcher/buttonfade-blue.png")`);
        await expect(wrapper).toHaveCSS('border-radius', '3px');
      }
      await page.getByRole('button', { name: `Unwatch thread ${id}`, exact: true }).last().press('Enter');
      for (let index = 0; index < 4; index++) await expect(controls.nth(index)).toHaveAttribute('aria-pressed', 'false');
      await expect(page.locator('#watchList li')).toHaveCount(0);
    }
  }
});

test('mobile navigation refresh reloads actual replies at the requested anchor and retains watches', async ({ page, context, request, createThread }) => {
  const id = await createThread('demo', 'Refresh navigation fixture');
  await page.setViewportSize({ width: 390, height: 844 });
  await context.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
    localStorage.setItem('4chan-tw-timestamp', String(Date.now()));
  });
  await page.goto(`/demo/thread/${id}`);
  await page.getByRole('button', { name: `Watch thread ${id}`, exact: true }).first().click();
  const response = await request.post('/demo/post', { headers: { Origin: origin },
    form: { resto: id, com: 'Reply fetched by native mobile refresh', password: 'watcher-test-password' }, maxRedirects: 0 });
  expect(response.status()).toBe(303);
  const reply = response.headers().location.match(/#p(\d+)/)[1];
  await expect(page.locator(`#p${reply}`)).toHaveCount(0);
  for (const target of ['bottom', 'top']) {
    const loaded = page.waitForResponse(response => response.request().isNavigationRequest()
      && new URL(response.url()).pathname === `/demo/thread/${id}`);
    await page.locator(`#refresh_${target}`).click();
    expect((await loaded).status()).toBe(200);
    await expect(page).toHaveURL(`${origin}/demo/thread/${id}#${target}`);
    await expect(page.locator(`#p${reply}`)).toBeVisible();
    await expect(page.getByRole('button', { name: `Unwatch thread ${id}`, exact: true })).toHaveCount(2);
    for (let index = 0; index < 4; index++) await expect(page.locator('.threadNav .wbtn').nth(index)).toHaveAttribute('aria-pressed', 'true');
  }
});

test('thread navigation keeps real return and refresh links without JavaScript', async ({ browser, createThread }) => {
  const id = await createThread('demo', 'No JavaScript navigation fixture');
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width: 390, height: 844 } });
  try {
    const page = await context.newPage();
    await page.goto(`${origin}/demo/thread/${id}`);
    await expect(page.locator('.threadNav:visible')).toHaveCount(2);
    await expect(page.locator('.wbtn')).toHaveCount(0);
    const loaded = page.waitForResponse(response => response.request().isNavigationRequest()
      && new URL(response.url()).pathname === `/demo/thread/${id}`);
    await page.locator('#refresh_top').click();
    expect((await loaded).status()).toBe(200);
    await page.getByRole('link', { name: 'Return', exact: true }).first().click();
    await expect(page).toHaveURL(`${origin}/demo/`);
    await expect(page.getByRole('heading', { name: 'Start a new thread' })).toBeVisible();
  } finally { await context.close(); }
});

test('board post menus watch persisted threads and synchronize an open menu across tabs', async ({ page, context, createThread }) => {
  const id = await createThread('demo', '<b>Menu watcher</b>');
  await context.addInitScript(() => {
    if (localStorage.getItem('4chan-settings') === null) {
      localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
    }
    localStorage.setItem('4chan-tw-timestamp', String(Date.now()));
  });
  await page.goto('/demo/');
  await expect(page.locator('.board .wbtn')).toHaveCount(0);
  const trigger = page.getByRole('button', { name: `Post menu for post ${id}`, exact: true });
  await trigger.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'Report post', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'Hide thread', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'Add to watch list', exact: true })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.locator(`#watch-${id}-demo`)).toContainText('<b>Menu watcher</b>');
  await expect(page.locator(`#watch-${id}-demo b`)).toHaveCount(0);
  await expect(page.locator('#post-menu')).toHaveCount(0);
  await expect(trigger).toBeFocused();
  const other = await context.newPage();
  await other.goto('/demo/');
  await trigger.click();
  await expect(page.getByRole('menuitem', { name: 'Remove from watch list', exact: true })).toBeVisible();
  await other.getByRole('button', { name: `Unwatch /demo/ thread ${id}`, exact: true }).click();
  await expect(page.getByRole('menuitem', { name: 'Add to watch list', exact: true })).toBeVisible();
  await expect(page.locator(`#watch-${id}-demo`)).toHaveCount(0);
  await saveWatcherSettings(other, { threadWatcher: false });
  await expect(page.locator('#post-menu [data-cmd="watch"]')).toHaveCount(0);
  await expect(page.getByRole('menuitem', { name: 'Report post', exact: true })).toBeVisible();
  await saveWatcherSettings(other, { threadWatcher: true });
  await expect(page.getByRole('menuitem', { name: 'Add to watch list', exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#post-menu')).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

test('post menus select the actual report and password-gated deletion forms without submitting on selection', async ({ page, request, createThread }) => {
  const id = await createThread('demo', 'Post action menu fixture');
  const replyResponse = await request.post('/demo/post', { headers: { Origin: origin },
    form: { resto: id, com: 'Reply selected through the native menu', password: 'watcher-test-password' }, maxRedirects: 0 });
  expect(replyResponse.status()).toBe(303);
  const reply = replyResponse.headers().location.match(/#p(\d+)/)[1];
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/demo/thread/${id}`);
  const trigger = page.getByRole('button', { name: `Post menu for post ${reply}`, exact: true });
  let writes = 0;
  page.on('request', request => { if (request.method() === 'POST') writes++; });
  await trigger.click();
  await expect(page.locator('#post-menu [data-cmd="watch"]')).toHaveCount(0);
  await page.getByRole('menuitem', { name: 'Report post', exact: true }).click();
  await expect(page.locator(`#report${reply}`)).toBeFocused();
  expect(writes).toBe(0);
  await page.locator(`#report${reply}`).fill('Owned post-menu report fixture');
  const reported = page.waitForResponse(response => response.request().method() === 'POST' && response.url().endsWith('/demo/report'));
  await page.locator(`#p${reply} form[action="/demo/report"] button`).click();
  expect((await reported).status()).toBe(200);
  await expect(page.getByText('Your report was saved. Staff review is not available in this development build.', { exact: true })).toBeVisible();
  await page.goto(`/demo/thread/${id}`);
  await trigger.click();
  await page.getByRole('menuitem', { name: 'Delete post', exact: true }).click();
  await expect(page.locator(`#delete${reply}`)).toBeFocused();
  expect(writes).toBe(1);
  await page.locator(`#delete${reply}`).fill('watcher-test-password');
  const deleted = page.waitForResponse(response => response.request().method() === 'POST' && response.url().endsWith('/demo/delete'));
  await page.locator(`#p${reply} form[action="/demo/delete"] button`).click();
  expect((await deleted).status()).toBe(303);
  const thread = await (await request.get(`/demo/thread/${id}.json`)).json();
  expect(thread.posts.some(post => String(post.no) === reply)).toBe(false);
});

test('post menus close on outside activation, Escape and viewport changes and honor global disabling', async ({ page, context, createThread }) => {
  const id = await createThread('demo', 'Post menu dismissal fixture');
  await page.goto(`/demo/thread/${id}`);
  const trigger = page.getByRole('button', { name: `Post menu for post ${id}`, exact: true });
  await trigger.press('Enter');
  await expect(page.getByRole('menuitem', { name: 'Report post', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  await trigger.click();
  await page.locator('h1').click();
  await expect(page.locator('#post-menu')).toHaveCount(0);
  await trigger.click();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator('#post-menu')).toHaveCount(0);
  await expect(trigger).toHaveText('...');
  await trigger.press('ArrowUp');
  await expect(page.getByRole('menuitem', { name: 'Delete post', exact: true })).toBeFocused();
  await page.keyboard.press('Home');
  await expect(page.getByRole('menuitem', { name: 'Report post', exact: true })).toBeFocused();
  const other = await context.newPage();
  await other.goto('/demo/');
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await expect(page.locator('#post-menu')).toHaveCount(0);
  await expect(trigger).toBeHidden();
  await other.evaluate(() => localStorage.setItem('4chan-settings', '{}'));
  await expect(trigger).toBeVisible();
});

test('post-menu watch changes remain usable when optional storage is unavailable', async ({ page, context, createThread }) => {
  const id = await createThread('demo', 'Volatile post-menu fixture');
  await context.addInitScript(() => { for (const method of ['getItem', 'setItem', 'removeItem']) Storage.prototype[method] = () => { throw new Error('Storage unavailable'); }; });
  await enable(page, '/demo/', { reload: false });
  const trigger = page.getByRole('button', { name: `Post menu for post ${id}`, exact: true });
  await trigger.click();
  await page.getByRole('menuitem', { name: 'Add to watch list', exact: true }).click();
  await expect(page.locator(`#watch-${id}-demo`)).toBeVisible();
  await trigger.click();
  await page.getByRole('menuitem', { name: 'Remove from watch list', exact: true }).click();
  await expect(page.locator(`#watch-${id}-demo`)).toHaveCount(0);
  await expect(page.locator('.watcherNotice')).toContainText('Changes stay in this tab');
});

test('watcher connect CSP permits its owned alias and denies healthy unrelated routes', async ({ page, context, request, createThread }) => {
  const id = await createThread('demo', 'CSP watcher fixture');
  let forbiddenRequests = 0;
  await context.route('**/watcher-connect-control', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><p>Owned positive control</p>' }));
  await context.route('**/watcher-denied', route => { forbiddenRequests++; return route.fulfill({ contentType: 'application/json', body: '{"ok":true}' }); });
  await page.goto('/watcher-connect-control');
  expect(await page.evaluate(() => fetch('/watcher-denied').then(response => response.json()))).toEqual({ ok: true });
  expect(forbiddenRequests).toBe(1);
  const response = await page.goto(`/demo/thread/${id}`);
  expect(response.headers()['content-security-policy']).toContain(`connect-src ${origin}/_watch/;`);
  expect(await page.evaluate(id => fetch(`/_watch/demo/thread/${id}.json`, { credentials: 'omit', redirect: 'error' }).then(response => response.json()).then(value => String(value.posts[0].no)), id)).toBe(id);
  expect(await page.evaluate(() => fetch('/watcher-denied').then(() => 'allowed', () => 'blocked'))).toBe('blocked');
  expect(forbiddenRequests).toBe(1);
  const publicApi = await request.get(`/demo/thread/${id}.json`);
  const watcherApi = await request.get(`/_watch/demo/thread/${id}.json`);
  expect(await watcherApi.json()).toEqual(await publicApi.json());
  expect(watcherApi.headers()['content-security-policy']).toContain("script-src 'none';");
  expect((await request.post(`/_watch/demo/thread/${id}.json`, { headers: { Origin: origin }, data: '' })).status()).toBe(405);
});

test('local storage failure keeps same-tab watch controls usable without executable labels', async ({ page, context, createThread }) => {
  const id = await createThread('demo', '<img src=x onerror=alert(1)>');
  await context.addInitScript(() => { for (const method of ['getItem', 'setItem', 'removeItem']) Storage.prototype[method] = () => { throw new Error('Storage unavailable'); }; });
  await enable(page, `/demo/thread/${id}`, { reload: false });
  await page.getByRole('button', { name: `Watch thread ${id}`, exact: true }).first().click();
  await expect(page.locator(`#watch-${id}-demo`)).toContainText('<img src=x onerror=alert(1)>');
  await expect(page.locator('#watchList img')).toHaveCount(0);
  await expect(page.locator('.watcherNotice')).toContainText('Changes stay in this tab');
  await page.getByRole('button', { name: `Unwatch /demo/ thread ${id}`, exact: true }).click();
  await expect(page.locator('#watchList > li')).toHaveCount(0);
});

test('disabling the watcher in another tab cancels an in-flight response', async ({ page, context, createThread }) => {
  const id = await createThread('demo', 'Cancellation fixture');
  await enable(page, '/demo/catalog?q=');
  await page.getByRole('button', { name: `Watch thread ${id}`, exact: true }).click();
  const other = await context.newPage();
  await other.goto('/demo/catalog?q=');
  let release;
  let started;
  const gate = new Promise(resolve => { release = resolve; });
  const requested = new Promise(resolve => { started = resolve; });
  await page.route(`**/_watch/demo/thread/${id}.json`, async route => {
    started(); await gate;
    try { await route.fulfill({ contentType: 'application/json', body: `{"posts":[{"no":${id},"resto":0},{"no":${BigInt(id) + 100000n},"resto":${id}}]}` }); } catch { /* The request was cancelled. */ }
  });
  await page.locator('#twPrune').click();
  await requested;
  await saveWatcherSettings(other, { threadWatcher: false });
  await expect(page.locator('#threadWatcher')).toBeHidden();
  release();
  await expect.poll(() => page.evaluate(id => JSON.parse(localStorage.getItem('4chan-watch'))[`${id}-demo`][2], id)).toBe(0);
});
