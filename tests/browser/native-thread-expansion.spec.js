import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000', password = 'owned-expansion-password';

test('index expansion preserves the tail and drafts while fetched replies retain real deletion actions', async ({ page, request }) => {
  test.setTimeout(90000);
  let thread;
  const replies = [];
  try {
    for (let index = 0; index < 9; index++) {
      const response = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0,
        form: { resto: thread || '0', password, com: `Owned expansion post ${index}.`, sub: index ? '' : 'Owned expansion thread' } }));
      expect(response.status()).toBe(303);
      const id = response.headers().location.split('#p')[1];
      if (index) replies.push(id); else thread = id;
    }
    await page.context().addCookies((await request.storageState()).cookies);
    await page.addInitScript(id => {
      localStorage.setItem('4chan-settings', JSON.stringify({ filter: true }));
      localStorage.setItem('4chan-filters', JSON.stringify([{ type: 2, pattern: '"Owned expansion post 2."', boards: 'fixture', active: true, color: '#ff0000' }]));
      window.expansionStates = [];
      document.addEventListener('4chanThreadExpanded', () => expansionStates.push(document.getElementById(`p${id}`)?.classList.contains('filter-hl')));
    }, replies[1]);
    await page.goto('/fixture/');
    const section = page.locator(`#t${thread}`);
    await expect(section.locator(':scope > .replyContainer')).toHaveCount(5);
    await section.locator(`#pc${replies.at(-1)} details.postActions`).evaluate(element => { element.open = true; });
    await section.locator(`#report${replies.at(-1)}`).fill('owned-tail-form-draft');
    await page.evaluate(thread => { window.ownedTail = [...document.getElementById(`t${thread}`).querySelectorAll(':scope > .postContainer')]; }, thread);
    const snapshots = []; let lastStarted = 0;
    page.on('request', request => { if (request.url().endsWith(`/_watch/fixture/thread/${thread}/posts`)) {
      snapshots.push(request.url()); lastStarted = Date.now();
    } });
    await section.getByRole('button', { name: `Expand thread ${thread}`, exact: true }).click();
    await expect(section.getByRole('button', { name: `Collapse thread ${thread}`, exact: true })).toBeVisible();
    await expect(section.locator(':scope > .replyContainer')).toHaveCount(8);
    await expect.poll(() => page.evaluate(() => expansionStates)).toEqual([true]);
    await expect(section.locator(`#p${replies[1]}`)).toHaveClass(/filter-hl/);
    expect(await page.evaluate(() => ownedTail.every(node => document.getElementById(node.id) === node))).toBe(true);
    await expect(section.locator(`#report${replies.at(-1)}`)).toHaveValue('owned-tail-form-draft');
    for (const id of replies.slice(0, 3)) {
      await expect(section.locator(`#m${id}`)).toBeVisible();
      await expect(section.locator(`#pi${id} > [data-post-menu]`)).toHaveCount(1);
    }
    await section.getByRole('button', { name: `Collapse thread ${thread}`, exact: true }).click();
    await expect(section.locator(':scope > .replyContainer:visible')).toHaveCount(5);
    await page.setViewportSize({ width: 390, height: 844 });
    await section.getByRole('button', { name: `Expand thread ${thread}`, exact: true }).click();
    await expect(section.locator(':scope > .replyContainer:visible')).toHaveCount(8);
    expect(snapshots).toHaveLength(1);
    await page.evaluate(() => {
      localStorage.setItem('4chan-settings', JSON.stringify({ filter: true, threadExpansion: false }));
      document.dispatchEvent(new Event('4chanSettingsSaved'));
    });
    await expect(section.locator(':scope > .replyContainer')).toHaveCount(5);
    await expect(section.locator('.rExpanded')).toHaveCount(0);
    expect(await page.evaluate(() => window.ownedTail.every(node => node.isConnected && document.getElementById(node.id) === node))).toBe(true);
    await page.evaluate(() => {
      localStorage.setItem('4chan-settings', JSON.stringify({ filter: true, threadExpansion: true }));
      document.dispatchEvent(new Event('4chanSettingsSaved'));
    });
    // The second fetch retains the production one-second request-start limit.
    await expect.poll(() => Date.now() - lastStarted).toBeGreaterThanOrEqual(1050);
    await section.getByRole('button', { name: `Expand thread ${thread}`, exact: true }).click();
    await expect(section.locator(':scope > .replyContainer')).toHaveCount(8);
    expect(snapshots).toHaveLength(2);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    const earlier = section.locator(`#pc${replies[0]}`);
    await earlier.locator('details.postActions').evaluate(element => { element.open = true; });
    await expect(earlier.locator(`#delete${replies[0]}`)).toHaveValue('');
    await withDeletionQuota(async () => {
      const deleted = page.waitForResponse(response =>
        new URL(response.url()).pathname === '/fixture/delete' && response.request().method() === 'POST');
      await earlier.getByRole('button', { name: 'Delete post', exact: true }).click();
      expect((await deleted).status()).toBe(303);
      await expect(page).toHaveURL(`${origin}/fixture/`);
      const body = await (await request.get(`/fixture/thread/${thread}.json`)).json();
      expect(body.posts.some(post => String(post.no) === replies[0])).toBe(false);
    });
  } finally {
    if (thread) await withDeletionQuota(async () => {
      expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0,
        form: { no: thread, password } })).status()).toBe(303);
    });
  }
});

test('script-free indexes keep the ordinary thread navigation', async ({ browser, request }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  try {
    const page = await context.newPage();
    await page.goto(`${origin}/fixture/`);
    await expect(page.locator('.nativeThreadExpand')).toHaveCount(0);
    const response = await request.get('/fixture/');
    expect(response.status()).toBe(200);
    expect((await response.text()).includes('nativeThreadExpand')).toBe(false);
  } finally { await context.close(); }
});
