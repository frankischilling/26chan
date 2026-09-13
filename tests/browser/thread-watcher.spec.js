import { test as base, expect } from '@playwright/test';

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
async function enable(page, path) {
  await page.goto(path);
  await page.locator('#thread-watcher-enable').click();
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
  await enable(page, `/demo/thread/${id}`);
  await page.getByRole('button', { name: `Watch thread ${id}`, exact: true }).click();
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
  await other.locator('#thread-watcher-enable').click();
  await expect(page.locator('#threadWatcher')).toBeHidden();
  release();
  await expect.poll(() => page.evaluate(id => JSON.parse(localStorage.getItem('4chan-watch'))[`${id}-demo`][2], id)).toBe(0);
});
