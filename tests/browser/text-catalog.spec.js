import { ownedDeletionMarker, cleanupDeletionFixtures } from './helpers/deletion-fixture.js';
import { fillCatalogSearch, applyCatalogSearch } from './catalog-actions.js';
import { test, expect } from '@playwright/test';

test('persisted text catalogs restore GET-excluded rows and sort live without navigation', async ({ browser, request }) => {
  const origin = 'http://127.0.0.1:3000', password = 'owned-text-catalog-password';
  const marker = ownedDeletionMarker();
  const created = [];
  const noScript = await browser.newContext({ javaScriptEnabled: false });
  const liveContext = await browser.newContext();
  await liveContext.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true })));
  const server = await noScript.newPage(), live = await liveContext.newPage();
  try {
    for (const subject of ['Alpha <script> & literal', 'Bravo', 'Crane']) {
      const response = await request.post('/news/post', { headers: { Origin: origin }, maxRedirects: 0,
        form: { sub: `${marker} ${subject}`, com: `${marker} comment\n\nsecond line`, password } });
      expect(response.status()).toBe(303);
      created.push(/#p(\d+)$/.exec(response.headers().location)[1]);
    }
    const reply = await request.post('/news/post', { headers: { Origin: origin }, maxRedirects: 0,
      form: { resto: created[0], com: 'Owned text catalog reply', password } });
    expect(reply.status()).toBe(303);
    const path = `${origin}/news/catalog?q=${encodeURIComponent(`${marker} Alpha`)}`;
    await server.goto(path);
    await live.goto(path);
    for (const page of [server, live]) {
      await expect(page.locator('#threads tbody > tr')).toHaveCount(1);
      const row = page.locator(`#threads #thread-${created[0]}`);
      await expect(row.locator('.txt-sub > a')).toHaveText(`${marker} Alpha <script> & literal`);
      await expect(row.locator('.txt-rep')).toHaveText('1');
      await expect(row.locator('img, script, .teaser')).toHaveCount(0);
      const data = await (await request.get(`/news/thread/${created[0]}.json`)).json();
      await expect(row.locator('.txt-date')).toHaveText(data.posts[0].now);
      expect(await page.locator('#catalogFiltered').evaluate(node => node.content.querySelectorAll('table > tbody > tr').length)).toBeGreaterThanOrEqual(2);
    }
    await expect(live.locator('#threadWatcher')).toBeVisible();
    await expect(live.locator('#threads .wbtn')).toHaveCount(0);
    let navigations = 0;
    live.on('request', request => { if (request.isNavigationRequest()) navigations++; });
    await fillCatalogSearch(live, marker);
    await applyCatalogSearch(live);
    const rows = live.locator('#threads tbody > tr');
    await expect(rows).toHaveCount(3);
    await live.locator('#order-ctrl').selectOption('date');
    expect(await rows.evaluateAll(nodes => nodes.map(node => node.dataset.threadId))).toEqual([...created].reverse());
    await live.locator('#order-ctrl').selectOption('r');
    expect(await rows.evaluateAll(nodes => nodes.map(node => node.dataset.threadId))).toEqual(created);
    await fillCatalogSearch(live, '^absent-owned-text-catalog$');
    await applyCatalogSearch(live);
    await expect(rows).toHaveCount(0);
    await expect(live.locator('#threads > .empty')).toBeVisible();
    await fillCatalogSearch(live, marker);
    await applyCatalogSearch(live);
    await expect(rows).toHaveCount(3);
    await expect(live.locator('#threads > .empty')).toHaveCount(0);
    await expect(rows.locator('.postMenuBtn')).toHaveCount(3);
    await expect(rows.locator('.wbtn')).toHaveCount(0);
    expect(navigations).toBe(0);
    for (const width of [480, 481]) {
      await live.setViewportSize({ width, height: 900 });
      await expect(rows.first().locator('.txt-date')).toHaveCSS('display', width === 480 ? 'none' : 'table-cell');
      await expect(rows.first().locator('.txt-ctrl')).toHaveCSS('display', width === 480 ? 'none' : 'table-cell');
    }
  } finally {
    try {
      cleanupDeletionFixtures(created.map((id, index) => ({ board: 'news', id, marker: `${marker} ${['Alpha <script> & literal', 'Bravo', 'Crane'][index]}` })));
    } finally { await noScript.close(); await liveContext.close(); }
  }
});
