import { watcherSettingsOpener } from './helpers/watcher-settings.js';
import { test, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import path from 'node:path';

const origin = 'http://127.0.0.1:3000';
const password = 'owned-depager-password';

function fixture(command, board) {
  const binary = path.resolve(process.env.CARGO_TARGET_DIR || 'target', `debug/examples/navigation-fixture${process.platform === 'win32' ? '.exe' : ''}`);
  const result = spawnSync(binary, [command, board], { encoding: 'utf8', timeout: 15000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot } });
  expect(result.error, 'Owned navigation fixture must launch').toBeUndefined();
  expect(result.status, 'Owned navigation fixture must succeed').toBe(0);
}

async function createThread(request, board, index) {
  const response = await request.post(`/${board}/post`, {
    headers: { Origin: origin }, maxRedirects: 0,
    form: { resto: '0', password, com: `Owned depager thread ${index}.`, sub: `Depager ${index}` },
  });
  expect(response.status()).toBe(303);
  return response.headers().location.split('#p')[1];
}

test('real board bootstrap drives desktop All, cancellation, mobile Load More and the alwaysDepage setting', async ({ page, request, context }) => {
  test.setTimeout(120000);
  const board = `dp${randomBytes(4).toString('hex')}`;
  fixture('setup', board);
  const threads = [];
  try {
    // A fresh board with two threads per page makes terminal-page behavior
    // independent of other tests and any existing user posts.
    for (let index = 0; index < 3; index++) threads.push(await createThread(request, board, index));
    await context.addCookies([{ name: 'private-session', value: 'synthetic-only', url: origin }]);
    const snapshots = [];
    page.on('request', req => {
      if (req.url().includes(`/_watch/${board}/page/`)) snapshots.push({ url: req.url(), headers: req.headers() });
    });

    await page.setViewportSize({ width: 1280, height: 480 });
    await page.goto(`/${board}/0`);
    const pageOneResponse = await request.get(`/_watch/${board}/page/1`);
    expect(pageOneResponse.status()).toBe(200);
    const pageOne = await pageOneResponse.json();
    expect(pageOne.threads.length).toBeGreaterThan(0);
    const pageOneThread = pageOne.threads[0].thread;
    const more = page.locator('#depage'), status = page.locator('#depage-status'), cancel = page.locator('#depage-cancel');
    await expect(page.locator('.nativeDepagerControls')).toHaveCount(1);
    await expect(more).toHaveCount(1);
    await expect(status).toHaveCount(1);
    await expect(cancel).toHaveCount(1);
    await expect(more).toBeVisible();
    await expect(more).toHaveText('All');
    await expect(more).toHaveAttribute('aria-label', 'Load more threads');
    await expect(more).toHaveAttribute('aria-pressed', 'false');
    await expect(status).toHaveAttribute('role', 'status');
    await expect(cancel).toBeHidden();

    await watcherSettingsOpener(page).first().click();
    await expect(page.getByLabel('Always use infinite scroll', { exact: true })).not.toBeChecked();
    await page.getByRole('button', { name: 'Close settings', exact: true }).click();

    await page.locator('#togglePostFormLink a').click();
    const postForm = page.locator('form.postEditor');
    await postForm.getByLabel('Deletion password', { exact: true }).fill('preserve-board-draft');
    await page.evaluate(() => {
      window.ownedDepagerOriginals = [...document.querySelectorAll('.board > .thread')];
      window.ownedNext = document.querySelector('nav.pages a[rel="next"]');
    });
    await more.click();
    await expect(more).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator(`#t${pageOneThread}`)).toBeVisible();
    await expect(status).toHaveText(pageOne.next_page === null ? ' Done.' : '');
    expect(snapshots.map(entry => entry.url)).toEqual([`${origin}/_watch/${board}/page/1`]);
    expect(snapshots[0].headers.cookie).toBeUndefined();
    await expect(postForm.getByLabel('Deletion password', { exact: true })).toHaveValue('preserve-board-draft');
    expect(await page.evaluate(() => ownedDepagerOriginals.every(node => node.isConnected && document.getElementById(node.id) === node))).toBe(true);
    expect(await page.evaluate(() => document.querySelector('nav.pages a[rel="next"]') === ownedNext)).toBe(true);

    await page.evaluate(() => {
      dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
      dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
    });
    await expect(page.locator('.nativeDepagerControls')).toHaveCount(1);
    await expect(page.locator(`#t${pageOneThread}`)).toHaveCount(1);
    expect(snapshots).toHaveLength(1);

    // A second desktop click must remain available after the last page arrived so
    // page-local auto can be switched off even though there is nothing left to fetch.
    await expect(more).toBeEnabled();
    await more.click();
    await expect(more).toHaveAttribute('aria-pressed', 'false');
    await page.waitForTimeout(150);
    expect(snapshots).toHaveLength(1);

    // Reload the real application and hold its actual page request. The bootstrap
    // owns both status text and the cancellation affordance.
    await page.reload();
    let releaseRoute;
    const release = new Promise(resolve => { releaseRoute = resolve; });
    let heldResolve;
    const held = new Promise(resolve => { heldResolve = resolve; });
    await page.route(`**/_watch/${board}/page/1`, async route => {
      heldResolve(route);
      await release;
      try { await route.continue(); } catch { /* The UI cancellation aborts this request. */ }
    });
    await page.locator('#depage').click();
    await held;
    await expect(page.locator('#depage-status')).toHaveText(' Loading next page...');
    await expect(page.locator('#depage-cancel')).toBeVisible();
    await page.locator('#depage-cancel').click();
    releaseRoute();
    await expect(page.locator('#depage-cancel')).toBeHidden();
    await expect(page.locator('.depageNumber')).toHaveCount(0);
    await page.unroute(`**/_watch/${board}/page/1`);

    // Mobile retains manual Load More without turning on infinite scrolling.
    await page.setViewportSize({ width: 390, height: 844 });
    await page.reload();
    await expect(page.locator('#depage')).toHaveText('Load More');
    await expect(page.locator('#depage')).toHaveAttribute('aria-pressed', 'false');
    const beforeMobile = snapshots.length;
    await page.locator('#depage').click();
    await expect(page.locator(`#t${pageOneThread}`)).toBeVisible();
    await expect(page.locator('#depage')).toHaveAttribute('aria-pressed', 'false');
    expect(snapshots).toHaveLength(beforeMobile + 1);
    expect(snapshots.at(-1).url).toBe(`${origin}/_watch/${board}/page/1`);

    // The persisted source override changes the effective layout without a
    // viewport change. Depager must follow that layout live as well.
    await page.reload();
    await expect(page.locator('#depage')).toHaveText('Load More');
    await page.evaluate(() => {
      localStorage.setItem('4chan_never_show_mobile', 'true');
      dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile' }));
    });
    await expect(page.locator('#depage')).toHaveText('All');
    await expect(page.locator('#depage')).toHaveAttribute('aria-pressed', 'false');

    const beforeNeverMobile = snapshots.length;
    await page.locator('#depage').click();
    await expect(page.locator('#depage')).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator(`#t${pageOneThread}`)).toBeVisible();
    expect(snapshots).toHaveLength(beforeNeverMobile + 1);
    await expect(page.locator('#depage')).toBeEnabled();

    await page.locator('#depage').click();
    await expect(page.locator('#depage')).toHaveAttribute('aria-pressed', 'false');
    await expect(page.locator('#depage')).toBeDisabled();
  } finally {
    try { for (const thread of threads.reverse()) {
      const response = await request.post(`/${board}/delete`, {
        headers: { Origin: origin }, maxRedirects: 0,
        form: { no: thread, password },
      });
      expect([303, 404]).toContain(response.status());
    } } finally { fixture('cleanup', board); }
  }
});

test('no-next and script-free board indexes keep ordinary pagination as the fallback', async ({ browser, page, request }) => {
  test.setTimeout(120000);
  const board = `dp${randomBytes(4).toString('hex')}`;
  fixture('setup', board);
  const threads = [];
  try {
    for (let index = 0; index < 3; index++) threads.push(await createThread(request, board, index));

    const malformed = await request.get(`/_watch/${board}/page/01`);
    expect(malformed.status()).toBe(404);

    // Three owned threads at two per page guarantee that page 1 is terminal.
    await page.goto(`/${board}/1`);
    await expect(page.locator('nav.pages [rel="next"]')).toHaveCount(0);
    await expect(page.locator('#depage')).toBeHidden();
    await expect(page.locator('#depage-status')).toBeHidden();

    const context = await browser.newContext({ javaScriptEnabled: false });
    try {
      const noScript = await context.newPage();
      await noScript.goto(`${origin}/${board}/0`);
      await expect(noScript.locator('#depage,#depage-status,#depage-cancel')).toHaveCount(0);
      await expect(noScript.locator('nav.pages a[rel="next"]')).toHaveAttribute('href', `/${board}/1`);
    } finally { await context.close(); }
  } finally {
    try { for (const thread of threads.reverse()) {
      const response = await request.post(`/${board}/delete`, {
        headers: { Origin: origin }, maxRedirects: 0,
        form: { no: thread, password },
      });
      expect([303, 404]).toContain(response.status());
    } } finally { fixture('cleanup', board); }
  }
});
