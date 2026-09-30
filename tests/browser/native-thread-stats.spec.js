import { test as base, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-thread-stats-password';
    const write = form => request.post('/demo/post', {
      headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password },
    });
    const created = await write({ resto: '0', sub: 'Owned thread stats', com: 'Original post' });
    expect(created.status()).toBe(303);
    const id = created.headers().location.match(/thread\/(\d+)/)[1];
    const remove = no => request.post('/demo/delete', {
      headers: { Origin: origin }, maxRedirects: 0, form: { no, password },
    });
    try {
      const reply = async com => {
        const response = await write({ resto: id, com });
        expect(response.status()).toBe(303);
        return response.headers().location.match(/#p(\d+)/)[1];
      };
      await reply('First owned stats reply');
      await reply('Second owned stats reply');
      await use({ id, url: `/demo/thread/${id}`, stats: `/_watch/demo/thread/${id}/stats` });
    } finally { await remove(id); }
  },
});

test('board index does not request thread statistics without a thread context', async ({ page }) => {
  const requests = [];
  page.on('request', request => {
    const path = new URL(request.url()).pathname;
    if (path.startsWith('/_watch/demo/thread/') && path.endsWith('/stats')) requests.push(path);
  });
  await page.goto('/demo/');
  await expect(page.locator('#settingsWindowLink:visible, #settingsWindowLinkMobile:visible')).toBeVisible();
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect(requests).toEqual([]);
  await expect(page.locator('.thread-stats')).toHaveCount(0);
});

test('real thread stats use the typed coherent endpoint and ignore synthetic DOM counts', async ({ page, request, owned }) => {
  const response = await request.get(owned.stats);
  expect(response.status()).toBe(200);
  expect(response.headers()['content-type']).toMatch(/^application\/json(?:;|$)/);
  const snapshot = await response.json();
  expect(Object.keys(snapshot).sort()).toEqual([
    'archived', 'board', 'bump_limited', 'closed', 'image_limited', 'images',
    'page', 'replies', 'sticky', 'thread', 'unique_ips', 'version',
  ].sort());
  expect(snapshot).toMatchObject({ version: 1, board: 'demo', thread: owned.id, replies: 2, images: 0,
    archived: false, sticky: false, closed: false, unique_ips: 1 });
  expect(snapshot.page).toBeGreaterThanOrEqual(1);
  expect(snapshot.page).toBeLessThanOrEqual(1000);
  expect(typeof snapshot.bump_limited).toBe('boolean');
  expect(typeof snapshot.image_limited).toBe('boolean');

  await page.goto(owned.url);
  await expect(page.locator('.threadNav.desktop .thread-stats')).toHaveCount(2);
  for (const node of await page.locator('.thread-stats').all()) {
    await expect(node.locator('.ts-replies')).toHaveText('2');
    await expect(node.locator('.ts-images')).toHaveText('0');
    await expect(node.locator('.ts-page')).toHaveText(String(snapshot.page));
    await expect(node.locator('.ts-ips')).toHaveText('1');
  }
  await expect(page.locator('.ts-ips')).toHaveCount(2);

  const refresh = page.waitForResponse(value => new URL(value.url()).pathname === owned.stats);
  await page.evaluate(() => {
    const fake = document.createElement('article');
    fake.className = 'replyContainer';
    fake.hidden = true;
    fake.innerHTML = '<span class="fileText">synthetic cloned file</span>';
    document.querySelector('.thread').append(fake);
    document.dispatchEvent(new Event('boardThreadStateChanged'));
  });
  await refresh;
  await expect(page.locator('.thread-stats .ts-replies').first()).toHaveText('2');
  await expect(page.locator('.thread-stats .ts-images').first()).toHaveText('0');
});

test('stats default on, follow cross-tab settings, and switch between desktop and one mobile placement', async ({ page, context, owned }) => {
  await page.goto(owned.url);
  await expect(page.locator('.threadNav.desktop .thread-stats')).toHaveCount(2);

  await page.setViewportSize({ width: 390, height: 800 });
  await expect(page.locator('.thread-stats')).toHaveCount(1);
  await expect(page.locator('.thread-stats .ts-ips')).toHaveText('1');
  await expect(page.locator('.threadNav.mobile[data-watch-position="bottom-mobile"] + .thread-stats')).toHaveCount(1);

  await page.setViewportSize({ width: 1024, height: 768 });
  await expect(page.locator('.threadNav.desktop .thread-stats')).toHaveCount(2);
  const other = await context.newPage();
  await other.goto(owned.url);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadStats: false })));
  await expect(page.locator('.thread-stats')).toHaveCount(0);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadStats: true })));
  await expect(page.locator('.threadNav.desktop .thread-stats')).toHaveCount(2);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadStats: true, disableAll: true })));
  await expect(page.locator('.thread-stats')).toHaveCount(0);
  await other.close();
});

test('thread page keeps its server navigation and content when JavaScript is disabled', async ({ browser, owned }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  try {
    await page.goto(`${origin}${owned.url}`);
    await expect(page.locator(`#t${owned.id}`)).toBeVisible();
    await expect(page.locator('.threadNav.desktop')).toHaveCount(2);
    await expect(page.locator('.thread-stats')).toHaveCount(0);
    await expect(page.getByText('Second owned stats reply', { exact: true })).toBeVisible();
  } finally { await context.close(); }
});
