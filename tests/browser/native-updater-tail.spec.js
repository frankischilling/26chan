import { test as base, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import path from 'node:path';
const origin = 'http://127.0.0.1:3000';
function fixture(command, slug) {
  const executable = path.resolve(`target/debug/examples/tail-fixture${process.platform === 'win32' ? '.exe' : ''}`);
  const result = spawnSync(executable, [command, slug], { encoding: 'utf8', timeout: 15000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot } });
  expect(result.status, 'Owned tail fixture helper must succeed').toBe(0);
}
const test = base.extend({
  owned: async ({ context }, use) => {
    const slug = `ut${randomBytes(4).toString('hex')}`, request = context.request, password = 'owned-updater-tail-password';
    fixture('setup', slug);
    try {
      const write = async (resto, com, track = false) => {
        const response = await request.post(`/${slug}/post`, { headers: { Origin: origin }, maxRedirects: 0,
          form: { resto, com, password, ...(resto === '0' ? { sub: 'Owned updater tail' } : {}), ...(track ? { track: '1' } : {}) } });
        expect(response.status()).toBe(303); return response.headers().location.match(/#p(\d+)/)[1];
      };
      const id = await write('0', 'Original post'), ids = [id];
      for (let i = 1; i <= 4; i++) ids.push(await write(id, `Existing reply ${i}`, i === 4));
      fixture('age', slug);
      await use({ slug, id, ids, url: `/${slug}/thread/${id}`, full: `/_watch/${slug}/thread/${id}/posts`,
        tail: `/_watch/${slug}/thread/${id}/posts-tail`, reply: com => write(id, com),
        remove: () => request.post(`/${slug}/delete`, { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } }),
      });
    } finally { fixture('cleanup', slug); }
  },
});
const status = page => page.locator('.threadNav.desktop .nativeUpdaterStatus').first();
const update = page => page.locator('.threadNav.desktop a[data-cmd=update]').first().click();
async function initialize(page, owned) {
  await page.addInitScript(() => {
    window.tailEvents = [];
    document.addEventListener('4chanThreadUpdated', event => window.tailEvents.push(event.detail.count));
  });
  await page.goto(owned.url);
  await expect(page.locator(`#t${owned.id}`)).toHaveAttribute('data-tail-size', '2');
  await expect(page.locator('.threadNav.desktop a[data-cmd=update]').first()).toBeVisible();
  const time = new Date(); await page.clock.install({ time }); await page.clock.pauseAt(time);
}
function traffic(page, owned) {
  const responses = [];
  page.on('response', response => { if ([owned.full, owned.tail].includes(new URL(response.url()).pathname)) responses.push(response); });
  return responses;
}
const postIds = (page, owned) => page.locator(`#t${owned.id} > .postContainer`).evaluateAll(posts => posts.map(post => post.id.slice(2)));

test('initial tail selection and conditional 304 retain drafts, and a real new tail reply is inserted once', async ({ page, owned }) => {
  await initialize(page, owned); const responses = traffic(page, owned);
  await page.locator('#togglePostFormLink a').click(); await page.locator('#com').fill('Retained tail draft'); await update(page);
  await expect(status(page)).toHaveText('No new posts'); expect(responses[0].status()).toBe(200);
  expect(new URL(responses[0].url()).pathname).toBe(owned.tail);
  await page.clock.runFor(1100); await update(page); await expect.poll(() => responses.length).toBe(2);
  await expect(status(page)).toHaveText('No new posts'); expect(responses[1].status()).toBe(304);
  expect(await page.evaluate(() => window.tailEvents)).toEqual([]);
  const reply = await owned.reply('Fresh <script> tail reply');
  await page.clock.runFor(1100); await update(page); await expect(status(page)).toHaveText('1 new post');
  expect(await postIds(page, owned)).toEqual([...owned.ids, reply]);
  await expect(page.locator(`#m${reply}`)).toHaveText('Fresh <script> tail reply');
  await expect(page.locator('#com')).toHaveValue('Retained tail draft');
  await expect(page.locator(`#p${reply} .postMenuBtn`)).toBeVisible();
  expect(await page.evaluate(() => window.tailEvents)).toEqual([1]);
  expect(responses.map(response => new URL(response.url()).pathname)).toEqual([owned.tail, owned.tail, owned.tail]);
});

test('a missing tail boundary fetches the real full response and inserts every missed reply', async ({ page, owned }) => {
  await initialize(page, owned); const responses = traffic(page, owned), added = [];
  for (let i = 0; i < 3; i++) added.push(await owned.reply(`Gap reply ${i}`));
  await update(page); await expect(status(page)).toHaveText('3 new posts');
  expect(responses.map(response => new URL(response.url()).pathname)).toEqual([owned.tail, owned.full]);
  expect(await postIds(page, owned)).toEqual([...owned.ids, ...added]);
  expect(await page.evaluate(() => window.tailEvents)).toEqual([3]);
  await page.clock.runFor(1100); await update(page); await expect.poll(() => responses.length).toBe(3);
  expect(new URL(responses[2].url()).pathname).toBe(owned.full); expect(responses[2].status()).toBe(304);
  await expect(status(page)).toHaveText('No new posts');
});

test('a removed tail falls back without marking the thread dead, while a later full 404 is terminal', async ({ page, owned }) => {
  await initialize(page, owned); const responses = traffic(page, owned);
  fixture('tail-off', owned.slug); await update(page); await expect(status(page)).toHaveText('No new posts');
  expect(responses.map(response => response.status())).toEqual([404, 200]);
  await expect(page.locator(`#t${owned.id}`)).toHaveAttribute('data-tail-size', '0');
  await page.clock.runFor(1100); await update(page); await expect.poll(() => responses.length).toBe(3);
  expect(new URL(responses[2].url()).pathname).toBe(owned.full); expect(responses[2].status()).toBe(304);
  expect((await owned.remove()).status()).toBe(303);
  await page.clock.runFor(1100); await update(page); await expect(status(page)).toHaveText('This thread has been pruned or deleted');
  expect(responses.at(-1).status()).toBe(404);
});

test('disabling during a held full fallback cancels the entire cycle and a fresh enabled update recovers the gap', async ({ page, context, owned }) => {
  await initialize(page, owned); const added = [];
  for (let i = 0; i < 3; i++) added.push(await owned.reply(`Cancelled gap ${i}`));
  let release, entered;
  const gate = new Promise(resolve => { release = resolve; }), held = new Promise(resolve => { entered = resolve; });
  await page.route(owned.full, async route => { entered(); await gate; await route.continue().catch(() => {}); }, { times: 1 });
  await update(page); await held;
  const other = await context.newPage(); await other.goto('/settings/theme');
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await expect(page.locator('.threadNav.desktop .nativeUpdater').first()).toBeHidden();
  release(); expect(await postIds(page, owned)).toEqual(owned.ids);
  await other.evaluate(() => localStorage.setItem('4chan-settings', '{}'));
  await expect(page.locator('.threadNav.desktop .nativeUpdater').first()).toBeVisible();
  await page.clock.runFor(1100); await update(page); await expect(status(page)).toHaveText('3 new posts');
  expect(await postIds(page, owned)).toEqual([...owned.ids, ...added]);
  expect(await page.evaluate(() => window.tailEvents)).toEqual([3]); await other.close();
});

test('automatic tail replies retain tracked-quote notifications and a later 304 does not double unread counts', async ({ page, owned }) => {
  await page.setViewportSize({ width: 1280, height: 400 }); await initialize(page, owned);
  const responses = traffic(page, owned), title = await page.title();
  const reply = await owned.reply(`>>${owned.ids[4]}\nAutomatic tracked tail`);
  await page.locator('.threadNav.desktop input[data-cmd=auto]').first().check();
  await page.clock.runFor(10000); await expect(page.locator(`#p${reply}`)).toBeVisible();
  await expect(page.locator(`#p${reply} .quotelink`)).toHaveClass(/ql-tracked/);
  await expect(page).toHaveTitle(`(1) ${title}`);
  await expect(page.locator('link[rel="shortcut icon"]')).toHaveAttribute('href', '/static/notifications/favicon-ws-newreplies.ico');
  await page.clock.runFor(10000); await expect.poll(() => responses.length).toBe(2);
  expect(responses[1].status()).toBe(304); await expect(page).toHaveTitle(`(1) ${title}`);
  expect(await page.evaluate(() => window.tailEvents)).toEqual([1]);
  expect(responses.map(response => new URL(response.url()).pathname)).toEqual([owned.tail, owned.tail]);
});

test('a real cross-origin API client reads and revalidates tails while an unapproved origin is denied', async ({ page, owned }) => {
  const api = `http://127.0.0.1:3003/${owned.slug}/thread/${owned.id}-tail.json`;
  const client = async host => {
    const url = `${host}/__owned-tail-api-client`;
    // Intercepted documents have unknown address space in Chromium. Permit only
    // this owned client to reach loopback; real API CORS remains enforced.
    await page.context().grantPermissions(['local-network-access'], { origin: host });
    await page.route(url, route => route.fulfill({ contentType: 'text/html',
      headers: { 'content-security-policy': "default-src 'none'; connect-src http://127.0.0.1:3003" },
      body: '<!doctype html><title>Owned tail API client</title>' }));
    await page.goto(url);
  };
  await client(origin);
  const result = await page.evaluate(async url => {
    const response = await fetch(url, { credentials: 'omit', cache: 'no-store' });
    const body = await response.json(), etag = response.headers.get('etag');
    const conditional = await fetch(url, { credentials: 'omit', cache: 'no-store', headers: { 'If-None-Match': etag } });
    return { status: response.status, boundary: String(body.posts[0].tail_id), etag,
      modified: response.headers.get('last-modified'), conditional: conditional.status, empty: await conditional.text() };
  }, api);
  expect(result.status).toBe(200); expect(result.boundary).toBe(owned.ids[2]);
  expect(result.etag).toMatch(/^"[0-9a-f]{64}"$/); expect(result.modified).toBeTruthy();
  expect(result.conditional).toBe(304); expect(result.empty).toBe('');
  await client('http://localhost:3000');
  expect(await page.evaluate(async url => { try { await fetch(url, { credentials: 'omit' }); return 'readable'; } catch (error) { return error.name; } }, api)).toBe('TypeError');
});
