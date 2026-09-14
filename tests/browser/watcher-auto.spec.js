import { test as base, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  fixture: async ({ request }, use) => {
    const created = [];
    const password = 'owned-auto-watcher-password';
    const tag = `Auto${Date.now().toString(36)}`;
    const post = async (board, subject, thread = '0') => {
      const response = await request.post(`/${board}/post`, { headers: { Origin: origin },
        form: { resto: thread, sub: subject, com: 'Synthetic automatic watcher fixture', password }, maxRedirects: 0 });
      expect(response.status()).toBe(303);
      const id = response.headers().location.match(thread === '0' ? /thread\/(\d+)/ : /#p(\d+)/)[1];
      if (thread === '0') created.push({ board, id });
      return id;
    };
    const control = await post('demo', `Control ${tag}`);
    const demo = await post('demo', `<b>${tag} & paper</b>`);
    const other = await post('test', `${tag} fold`);
    const reply = await post('demo', '', demo);
    const filters = [
      { type: 5, pattern: `${tag} paper`, boards: 'demo', active: true, auto: true },
      { type: 5, pattern: `${tag} fold`, boards: 'test', active: true, auto: true },
    ];
    await use({ tag, control, demo, other, reply, filters, post });
    for (const { board, id } of created) {
      expect((await request.post(`/${board}/delete`, { headers: { Origin: origin },
        form: { no: id, password }, maxRedirects: 0 })).status()).toBe(303);
      expect((await request.get(`/_watch/${board}/thread/${id}.json`)).status()).toBe(404);
    }
  },
});

async function prepare(page, fixture, blacklist = {}) {
  await page.goto(`/demo/thread/${fixture.control}`);
  // Native stored preferences exercise integration independently of the editor,
  // which remains a separate unfinished requirement.
  await page.evaluate(({ filters, blacklist }) => {
    localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true, filter: true }));
    localStorage.setItem('4chan-filters', JSON.stringify(filters));
    localStorage.setItem('4chan-watch-bl', JSON.stringify(blacklist));
    localStorage.removeItem('4chan-tw-timestamp');
  }, { filters: fixture.filters, blacklist });
  await page.reload();
  await expect(page.locator('#threadWatcher')).toBeVisible();
}

async function refresh(page) {
  await page.locator('#twPrune').click();
  await expect(page.locator('.watcherNotice')).toHaveText(/^(?:Refresh complete\.|Some automatic watches could not be refreshed\..*|Storage or cross-tab locking is unavailable\..*)$/);
  await expect(page.locator('#threadWatcher')).toHaveAttribute('aria-busy', 'false');
}

test('manual extension refresh discovers two boards, counts replies and retains explicit unwatch blacklists', async ({ page, request, fixture }) => {
  const requests = [];
  page.on('request', request => { if (request.url().includes('/_watch/')) requests.push(request.url()); });
  await prepare(page, fixture);
  expect(requests).toHaveLength(0);
  await expect(page.locator('#watchList li')).toHaveCount(0);
  const source = await request.get(`/_watch/demo/thread/${fixture.demo}.json`);
  expect(source.status()).toBe(200);
  expect((await source.json()).posts[0].sub).toBe(`<b>${fixture.tag} & paper</b>`);
  await refresh(page);
  await expect(page.locator(`#watch-${fixture.demo}-demo a`)).toHaveText(`(1) /demo/ - ${fixture.tag} & paper`);
  await expect(page.locator(`#watch-${fixture.other}-test a`)).toHaveText(`/test/ - ${fixture.tag} fold`);
  await expect(page.locator('#watchList b, #watchList img')).toHaveCount(0);
  expect(requests.slice(0, 2).map(url => new URL(url).pathname)).toEqual(['/_watch/demo/catalog.json', '/_watch/test/catalog.json']);
  await page.getByRole('button', { name: `Unwatch /demo/ thread ${fixture.demo}`, exact: true }).click();
  await expect(page.locator(`#watch-${fixture.demo}-demo`)).toHaveCount(0);
  expect(await page.evaluate(id => JSON.parse(localStorage.getItem('4chan-watch-bl'))[`${id}-demo`], fixture.demo)).toBe(1);
  await page.evaluate(() => localStorage.removeItem('4chan-tw-timestamp'));
  await page.reload();
  await refresh(page);
  await expect(page.locator(`#watch-${fixture.demo}-demo`)).toHaveCount(0);
  await expect(page.locator(`#watch-${fixture.other}-test`)).toBeVisible();
});

test('failed board catalogs preserve suppression while successful boards add real matching threads', async ({ page, request, fixture }) => {
  const blocked = { [`${fixture.other}-test`]: 1, [`${fixture.demo}-test`]: 1 };
  expect((await request.get('/_watch/test/catalog.json')).status()).toBe(200);
  await page.route('**/_watch/test/catalog.json', route => route.fulfill({ status: 503, contentType: 'application/json', body: '{}' }));
  await prepare(page, fixture, blocked);
  await refresh(page);
  await expect(page.locator(`#watch-${fixture.demo}-demo`)).toBeVisible();
  await expect(page.locator(`#watch-${fixture.other}-test`)).toHaveCount(0);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-watch-bl')))).toEqual(blocked);
  await expect(page.locator('.watcherNotice')).toContainText('failed-board blacklists were retained');
  await page.unroute('**/_watch/test/catalog.json');
  await page.evaluate(() => localStorage.removeItem('4chan-tw-timestamp'));
  await page.reload();
  await refresh(page);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-watch-bl')))).toEqual({ [`${fixture.other}-test`]: 1 });
});

test('cross-tab filter changes cancel held catalogs without adding threads or pruning blacklists', async ({ page, context, fixture }) => {
  const blocked = { [`${fixture.other}-test`]: 1 };
  await prepare(page, fixture, blocked);
  const other = await context.newPage();
  await other.goto(`/demo/thread/${fixture.control}`);
  let release, started;
  const gate = new Promise(resolve => { release = resolve; });
  const requested = new Promise(resolve => { started = resolve; });
  await page.route('**/_watch/demo/catalog.json', async route => {
    const response = await route.fetch();
    started(response.status());
    await gate;
    try { await route.fulfill({ response }); } catch { /* The owned request was cancelled. */ }
  });
  try {
    await page.locator('#twPrune').click();
    expect(await requested).toBe(200);
    await other.evaluate(() => localStorage.setItem('4chan-filters', '[]'));
    await expect(page.locator('#threadWatcher')).toHaveAttribute('aria-busy', 'false');
    await expect(page.locator('.watcherNotice')).toHaveText('Refresh stopped.');
    release();
    await page.unrouteAll({ behavior: 'wait' });
    await expect(page.locator('#watchList li')).toHaveCount(0);
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-watch-bl')))).toEqual(blocked);
  } finally { release(); }
});

test('unavailable writes retain automatic watches and unwatch suppression in the current tab', async ({ page, fixture }) => {
  await prepare(page, fixture);
  await page.evaluate(() => { Storage.prototype.setItem = () => { throw new Error('Owned unavailable storage'); }; });
  await refresh(page);
  await expect(page.locator(`#watch-${fixture.demo}-demo`)).toBeVisible();
  await expect(page.locator('.watcherNotice')).toContainText('Changes stay in this tab');
  await page.getByRole('button', { name: `Unwatch /demo/ thread ${fixture.demo}`, exact: true }).click();
  await expect(page.locator(`#watch-${fixture.demo}-demo`)).toHaveCount(0);
  expect(await page.evaluate(() => localStorage.getItem('4chan-watch'))).toBeNull();
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-watch-bl')))).toEqual({});
});

test('malformed suppression state cannot silently allow automatic watching or forget an explicit unwatch', async ({ page, fixture }) => {
  await prepare(page, fixture, { invalid: 1 });
  await page.getByRole('button', { name: `Watch thread ${fixture.control}`, exact: true }).first().click();
  await refresh(page);
  await expect(page.locator('#watchList li')).toHaveCount(1);
  await expect(page.locator('.watcherNotice')).toContainText('Some automatic watches could not be refreshed');
  await page.getByRole('button', { name: `Unwatch /demo/ thread ${fixture.control}`, exact: true }).click();
  await expect(page.locator(`#watch-${fixture.control}-demo`)).toBeVisible();
  await expect(page.locator('.watcherNotice')).toHaveText('Watch blacklist is invalid. The watch was retained.');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-watch-bl')))).toEqual({ invalid: 1 });
});

test('a filter edit while automatic additions wait for the storage lock prevents their commit', async ({ page, context, fixture }) => {
  await prepare(page, fixture);
  const other = await context.newPage();
  await other.goto(`/demo/thread/${fixture.control}`);
  // Delay only the candidate commit (the second lock request), retaining the
  // native Web Lock and the real catalog/worker path underneath this wrapper.
  await page.evaluate(() => {
    const request = navigator.locks.request.bind(navigator.locks);
    let count = 0;
    navigator.locks.request = async (...args) => {
      if (++count === 2) {
        window.autoCommitWaiting = true;
        await new Promise(resolve => { window.releaseAutoCommit = resolve; });
      }
      return request(...args);
    };
  });
  try {
    await page.locator('#twPrune').click();
    await expect.poll(() => page.evaluate(() => window.autoCommitWaiting)).toBe(true);
    await other.evaluate(() => localStorage.setItem('4chan-filters', '[]'));
    await page.evaluate(() => window.releaseAutoCommit());
    await expect(page.locator('#threadWatcher')).toHaveAttribute('aria-busy', 'false');
    await expect(page.locator('.watcherNotice')).toHaveText('Refresh stopped.');
    await expect(page.locator('#watchList li')).toHaveCount(0);
    expect(await page.evaluate(() => localStorage.getItem('4chan-watch'))).toBeNull();
  } finally { await page.evaluate(() => window.releaseAutoCommit?.()); }
});

test('catalog and thread responses share the configured aggregate byte ceiling', async ({ page, fixture }) => {
  const extra = await fixture.post('demo', `${fixture.tag} paper extra`);
  await prepare(page, fixture);
  const sources = [];
  const responseBytes = 4 * 1024 * 1024;
  await page.route('**/_watch/**', async route => {
    const response = await route.fetch();
    expect(response.status()).toBe(200);
    const body = await response.body();
    expect(body.length).toBeLessThan(responseBytes);
    // Preserve actual owned JSON and add only bounded trailing JSON whitespace.
    sources.push(new URL(route.request().url()).pathname);
    await route.fulfill({ status: 200, contentType: 'application/json',
      body: Buffer.concat([body, Buffer.alloc(responseBytes - body.length, 0x20)]) });
  });
  await page.locator('#twPrune').click();
  await expect(page.locator('.watcherNotice')).toHaveText(/^[1-3] thread refreshes failed\. Saved state was retained\.$/, { timeout: 15000 });
  await expect(page.locator('#threadWatcher')).toHaveAttribute('aria-busy', 'false');
  await expect(page.locator('#watchList li')).toHaveCount(3);
  expect(sources.filter(path => path.endsWith('/catalog.json'))).toHaveLength(2);
  // Independent thread-phase accounting would allow all three 4 MiB thread
  // responses. This failure establishes that catalog bytes count toward it.
  expect(sources.filter(path => path.includes('/thread/')).length).toBeGreaterThanOrEqual(2);
  const control = await page.evaluate(async keys => {
    const { WatcherRefresh } = await import('/static/thread-watcher-core.v1.js');
    const entries = new Map(keys.map(key => [key, { label: 'Owned budget control', read: '0',
      unread: 0, archived: false, ownReply: false }]));
    return new WatcherRefresh({ origin: location.origin, getEntries: () => entries, commit: () => true }).refresh();
  }, [`${fixture.demo}-demo`, `${fixture.other}-test`, `${extra}-demo`]);
  expect(control.status).toBe('complete');
  expect(control.results.map(row => row.status)).toEqual(['updated', 'updated', 'updated']);
});
