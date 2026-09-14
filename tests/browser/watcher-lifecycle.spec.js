import { test as base, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const password = 'owned-watcher-lifecycle-password';
const test = base.extend({
  owned: async ({ request }, use) => {
    const threads = new Set();
    const posts = new Set();
    const write = (path, form) => request.post(`/demo/${path}`, {
      headers: { Origin: origin }, form: { ...form, password }, maxRedirects: 0,
    });
    const remove = async id => {
      expect(posts.has(id)).toBe(true);
      expect((await write('delete', { no: id })).status()).toBe(303);
      threads.delete(id);
      posts.delete(id);
    };
    await use({
      async thread(label) {
        const response = await write('post', { resto: '0', sub: label, com: 'Owned lifecycle fixture' });
        expect(response.status()).toBe(303);
        const id = response.headers().location.match(/thread\/(\d+)/)[1];
        threads.add(id);
        posts.add(id);
        return id;
      },
      async reply(thread) {
        expect(threads.has(thread)).toBe(true);
        const response = await write('post', { resto: thread, com: 'Owned unread reply' });
        expect(response.status()).toBe(303);
        const id = response.headers().location.match(/#p(\d+)/)[1];
        posts.add(id);
        return id;
      },
      remove,
    });
    // Only this test's remaining OPs are removed, through the password gate.
    for (const id of threads) {
      await remove(id);
      expect((await request.get(`/_watch/demo/thread/${id}.json`)).status()).toBe(404);
    }
  },
});

async function watch(page, id) {
  await page.goto('/demo/catalog?q=');
  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
    localStorage.removeItem('4chan-tw-timestamp');
  });
  await page.reload();
  await page.getByRole('button', { name: `Watch thread ${id}`, exact: true }).click();
  await expect(page.locator(`#watch-${id}-demo`)).toBeVisible();
}

async function prepareRefresh(page) {
  // A fresh client and absent catalog timestamp avoid shortening runtime limits.
  await page.evaluate(() => localStorage.removeItem('4chan-tw-timestamp'));
  await page.reload();
  await expect(page.locator('#threadWatcher')).toHaveAttribute('aria-busy', 'false');
}

async function refresh(page) {
  await prepareRefresh(page);
  await page.locator('#twPrune').click();
  // The initial idle state can precede acquisition of the cross-tab lock.
  await expect(page.locator('.watcherNotice')).toHaveText(
    /^(?:Refresh complete\.|[0-9]+ thread refreshes failed\. Saved state was retained\.)$/,
  );
  await expect(page.locator('#threadWatcher')).toHaveAttribute('aria-busy', 'false');
}

const saved = (page, id) => page.evaluate(id =>
  JSON.parse(localStorage.getItem('4chan-watch'))[`${id}-demo`] ?? null, id);

test('real reply deletion retains unread counts and OP deletion becomes dead before removal', async ({ page, request, owned }) => {
  const id = await owned.thread('Deleted watcher lifecycle');
  await watch(page, id);
  const reply = await owned.reply(id);
  await refresh(page);
  await expect(page.locator(`#watch-${id}-demo a`)).toHaveText('(1) /demo/ - Deleted watcher lifecycle');
  expect((await saved(page, id))[2]).toBe(1);

  await owned.remove(reply);
  const live = await request.get(`/_watch/demo/thread/${id}.json`);
  expect(live.status()).toBe(200);
  expect((await live.json()).posts.map(post => String(post.no))).toEqual([id]);
  await refresh(page);
  expect((await saved(page, id))[2]).toBe(1);
  await expect(page.locator(`#watch-${id}-demo a`)).toHaveClass(/hasNewReplies/);

  await owned.remove(id);
  expect((await request.get(`/_watch/demo/thread/${id}.json`)).status()).toBe(404);
  await refresh(page);
  const dead = await saved(page, id);
  expect(dead[1]).toBe(-1);
  expect(dead[2]).toBe(1);
  await expect(page.locator(`#watch-${id}-demo a`)).toHaveClass('deadlink');
  await expect(page.locator(`#watch-${id}-demo a`)).toHaveText('/demo/ - Deleted watcher lifecycle');

  let requests = 0;
  page.on('request', request => {
    if (request.url().endsWith(`/_watch/demo/thread/${id}.json`)) requests++;
  });
  await refresh(page);
  await expect(page.locator(`#watch-${id}-demo`)).toHaveCount(0);
  expect(await saved(page, id)).toBeNull();
  expect(requests).toBe(0);
});

test('failed and malformed owned responses preserve stored tuples and permit a later healthy refresh', async ({ page, request, owned }) => {
  const id = await owned.thread('Failed watcher lifecycle');
  await watch(page, id);
  await owned.reply(id);
  const endpoint = `**/_watch/demo/thread/${id}.json`;
  const healthy = await request.get(`/_watch/demo/thread/${id}.json`);
  expect(healthy.status()).toBe(200);
  expect((await healthy.json()).posts).toHaveLength(2);
  const before = await saved(page, id);
  for (const response of [
    { status: 503, contentType: 'application/json', body: '{}' },
    { status: 200, contentType: 'text/html', body: '<p>Not a thread</p>' },
    { status: 200, contentType: 'application/json', body: '{"posts":[]}' },
    { status: 200, contentType: 'application/json', body: Buffer.from([0xff]) },
  ]) {
    await page.route(endpoint, route => route.fulfill(response));
    try {
      await refresh(page);
      await expect(page.locator('.watcherNotice')).toContainText('1 thread refreshes failed');
      expect(await saved(page, id)).toEqual(before);
      await expect(page.locator(`#watch-${id}-demo a`)).not.toHaveClass(/deadlink/);
    } finally { await page.unroute(endpoint); }
  }
  await refresh(page);
  await expect(page.locator('.watcherNotice')).toHaveText('Refresh complete.');
  expect((await saved(page, id))[2]).toBe(1);
});

for (const action of ['unwatch', 'acknowledge']) {
  test(`a late real response cannot overwrite a cross-tab ${action}`, async ({ page, context, owned }) => {
    const id = await owned.thread(`Delayed ${action} lifecycle`);
    // Deliberately ignore cancellation in the owned transport to force delivery
    // after the storage event, exercising stale-result rejection independently.
    await page.addInitScript(() => {
      const fetch = window.fetch.bind(window);
      window.lifecycleDelivered = 0;
      window.fetch = async (url, options) => {
        if (typeof url !== 'string' || !url.includes('/_watch/demo/thread/')) return fetch(url, options);
        const response = await fetch(url, { ...options, signal: undefined });
        window.lifecycleDelivered++;
        return response;
      };
    });
    await watch(page, id);
    const reply = await owned.reply(id);
    const other = await context.newPage();
    await other.goto('/demo/catalog?q=');
    await expect(other.locator(`#watch-${id}-demo`)).toBeVisible();
    await prepareRefresh(page);
    let release, arrived;
    const gate = new Promise(resolve => { release = resolve; });
    const requested = new Promise(resolve => { arrived = resolve; });
    const endpoint = `**/_watch/demo/thread/${id}.json`;
    await page.route(endpoint, async route => {
      const response = await route.fetch();
      const body = await response.body();
      arrived({ status: response.status(), posts: JSON.parse(body.toString()).posts });
      await gate;
      await route.fulfill({ response, body });
    });
    try {
      await page.locator('#twPrune').click();
      const response = await requested;
      expect(response.status).toBe(200);
      expect(response.posts.map(post => String(post.no))).toEqual([id, reply]);
      if (action === 'unwatch') {
        await other.getByRole('button', { name: `Unwatch /demo/ thread ${id}`, exact: true }).click();
        await expect(page.locator(`#watch-${id}-demo`)).toHaveCount(0);
      } else {
        await other.goto(`/demo/thread/${id}`);
        await expect(page.locator(`#watch-${id}-demo a`)).toHaveAttribute('href', `/demo/thread/${id}#p${reply}`);
      }
      const current = await saved(other, id);
      release();
      await expect.poll(() => page.evaluate(() => window.lifecycleDelivered)).toBe(1);
      await expect(page.locator('#threadWatcher')).toHaveAttribute('aria-busy', 'false');
      expect(await saved(page, id)).toEqual(current);
      expect(await saved(other, id)).toEqual(current);
      if (action === 'unwatch') {
        expect(current).toBeNull();
        await expect(page.locator(`#watch-${id}-demo`)).toHaveCount(0);
      } else {
        expect(String(current[1])).toBe(reply);
        expect(current[2]).toBe(0);
        await expect(page.locator(`#watch-${id}-demo a`)).not.toHaveClass(/hasNewReplies/);
      }
    } finally {
      release();
      await page.unrouteAll({ behavior: 'wait' });
    }
  });
}
