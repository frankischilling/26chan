import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';
import { randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { readFileSync } from 'node:fs';

const navigationReference = JSON.parse(readFileSync(new URL('../../fixtures/navigation-reference.json', import.meta.url), 'utf8'));
const sourceHeaderGroups = navigationReference.header_groups;
const headerNws = new Map(navigationReference.header_parent_nws[navigationReference.configured_header]);

const origin = 'http://127.0.0.1:3000';
const preference = '4chan_never_show_mobile';

function archiveFixture(command, slug) {
  const binary = process.platform === 'win32' ? '.exe' : '';
  const result = spawnSync(path.resolve(process.env.CARGO_TARGET_DIR || 'target', `debug/examples/archive-fixture${binary}`), [command, slug], {
    encoding: 'utf8', timeout: 15_000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot },
  });
  expect(result.error, 'Owned archive fixture helper must launch').toBeUndefined();
  expect(result.status, 'Owned archive fixture helper must succeed').toBe(0);
}

for (const catalog of [false, true]) {
  test(`script-free ${catalog ? 'catalog' : 'index'} navigation uses source header groups and Settings fallback`, async ({ browser, request }) => {
    const response = await request.get('/_watch/boards'); expect(response.status()).toBe(200);
    const directory = (await response.json()).boards;
    expect(directory.some(board => board.board === 'fixture')).toBe(true);
    const context = await browser.newContext({ javaScriptEnabled: false });
    try {
      const page = await context.newPage(); await page.goto(`${origin}/fixture/${catalog ? 'catalog' : ''}`);
      const expected = sourceHeaderGroups.map(group => group.map(([slug, title]) => ({
        slug, title, href: `/${slug}/${catalog && slug !== 'f' ? 'catalog' : ''}`, nws: headerNws.get(slug),
      })));
      for (const parent of ['#boardNavDesktop', '#boardNavDesktopFoot']) {
        await expect(page.locator(`${parent} [data-public-board-list]`)).toHaveCount(1);
        expect(await page.locator(`${parent} [data-public-board-group]`).evaluateAll(groups => groups.map(group =>
          [...group.querySelectorAll('a')].map(node => ({ slug: node.textContent, title: node.title, href: node.getAttribute('href'), nws: node.parentElement.classList.contains('nwsb') }))))).toEqual(expected);
      }
      expect(await page.locator('#boardSelectMobile option:not([data-current-board-fallback])').evaluateAll(nodes => nodes.map(node => [node.value, node.textContent, node.className]))).toEqual(
        sourceHeaderGroups.flat().sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([slug, title]) => [slug, `/${slug}/ - ${title}`, headerNws.get(slug) ? 'nwsb' : '']));
      await expect(page.locator('#boardSelectMobile option[data-current-board-fallback]')).toHaveValue('fixture');
      expect(directory.some(board => board.board === 'demo')).toBe(true);
      expect(sourceHeaderGroups.flat().some(([slug]) => slug === 'demo')).toBe(false);
      await expect(page.locator('#boardSelectMobile')).toHaveValue('fixture');
      for (const id of ['boardNavDesktop', 'boardNavMobile', 'boardNavDesktopFoot', 'navtopright', 'navbotright', 'settingsWindowLink', 'settingsWindowLinkMobile', 'settingsWindowLinkBot', 'bottom']) {
        await expect(page.locator(`#${id}`)).toHaveCount(1);
      }
      expect(await page.evaluate(() => document.getElementById('boardNavDesktopFoot').nextElementSibling.id)).toBe('absbot');
      const worksafe = await page.locator('body').getAttribute('data-worksafe');
      await page.locator('#settingsWindowLink').click();
      await expect(page).toHaveURL(`${origin}/settings/theme?worksafe=${worksafe}`);
      await expect(page.locator('form[action="/settings/theme"]')).toBeVisible();
      expect((await context.request.get(page.url())).status()).toBe(200);
    } finally { await context.close(); }
  });
}

test('thread Settings reuses all three server links without duplicate handlers or storage writes', async ({ page, context, request }) => {
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  const password = 'owned-chrome-thread-password';
  const created = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0,
    form: { resto: '0', sub: 'Owned navigation thread', com: 'Owned page navigation fixture.', password } }));
  expect(created.status()).toBe(303);
  const location = created.headers().location.split('#')[0], id = location.split('/').at(-1);
  await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  try {
    await page.goto(location);
    await expect(page.locator('[data-native-settings-ready]')).toHaveCount(3);
    const before = await page.evaluate(() => localStorage.getItem('4chan-settings'));
    for (const selector of ['#settingsWindowLink', '#settingsWindowLinkBot', '#settingsWindowLinkMobile']) {
      await page.setViewportSize({ width: selector.endsWith('Mobile') ? 390 : 1280, height: 900 });
      await page.locator(selector).click();
      const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
      await expect(dialog).toHaveCount(1); await expect(dialog).toBeVisible();
      await dialog.getByRole('button', { name: 'Close settings', exact: true }).click();
      await expect(dialog).toHaveCount(0); await expect(page.locator(selector)).toBeFocused();
    }
    expect(await page.evaluate(() => localStorage.getItem('4chan-settings'))).toBe(before);
    expect(errors).toEqual([]);
  } finally {
    await withDeletionQuota(async () => {
      expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303);
    });
  }
});

test('mobile catalog navigation preserves the view, reloads the saved mode and follows another tab', async ({ page, context }) => {
  const errors = [], external = []; page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (new URL(request.url()).origin !== origin) external.push(request.url()); });
  await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await page.setViewportSize({ width: 390, height: 900 }); await page.goto('/fixture/catalog');
  await expect(page.locator('body')).toHaveAttribute('data-native-never-mobile', 'false');
  await page.locator('#boardSelectMobile').selectOption('a'); await expect(page).toHaveURL(`${origin}/a/catalog`);
  await expect(page.locator('#settingsWindowLink')).toHaveAttribute('data-native-settings-ready', '');
  await Promise.all([page.waitForEvent('load'), page.locator('#boardNavMobile [data-page-mobile="disable"]').click()]);
  expect(await page.evaluate(key => localStorage.getItem(key), preference)).toBe('true');
  await expect(page.locator('#boardNavDesktop')).toBeVisible(); await expect(page.locator('#boardNavMobile')).toBeHidden();
  await Promise.all([page.waitForEvent('load'), page.locator('#navtopright [data-page-mobile="enable"]').click()]);
  expect(await page.evaluate(key => localStorage.getItem(key), preference)).toBeNull();
  await expect(page.locator('#boardNavMobile')).toBeVisible(); await expect(page.locator('#boardNavDesktop')).toBeHidden();
  const other = await context.newPage();
  try {
    await other.goto('/demo/');
    await other.evaluate(key => localStorage.setItem(key, 'true'), preference);
    await expect(page.locator('#boardNavMobile')).toBeHidden(); await expect(page.locator('#boardNavDesktop')).toBeVisible();
    await other.evaluate(key => localStorage.removeItem(key), preference);
    await expect(page.locator('#boardNavMobile')).toBeVisible(); await expect(page.locator('#boardNavDesktop')).toBeHidden();
  } finally { await other.close(); }
  expect(errors).toEqual([]); expect(external).toEqual([]);
});

test('a real archive admits page navigation while denying watcher code and fetches', async ({ browser }) => {
  const slug = `z${randomBytes(5).toString('hex').slice(0, 9)}`;
  archiveFixture('setup', slug);
  const context = await browser.newContext({ viewport: { width: 390, height: 900 } });
  const errors = [], watcherRequests = [];
  try {
    const page = await context.newPage(); page.on('pageerror', error => errors.push(error.message));
    page.on('request', request => { if (new URL(request.url()).pathname.startsWith('/_watch/')) watcherRequests.push(request.url()); });
    const response = await page.goto(`${origin}/${slug}/archive`); expect(response.status()).toBe(200);
    const policy = response.headers()['content-security-policy'].split(';').map(value => value.trim());
    expect(policy).toContain(`script-src ${origin}/static/page-chrome.v1.js`);
    for (const name of ['connect-src', 'worker-src', 'media-src', 'frame-src']) expect(policy).toContain(`${name} 'none'`);
    await expect(page.locator('body')).toHaveAttribute('data-native-never-mobile', 'false');
    await expect(page.locator('[data-native-settings-ready], .nativePersistentNavigation, #threadWatcher')).toHaveCount(0);
    await expect(page.locator('#bottom')).toHaveCount(1);
    expect(watcherRequests).toEqual([]);
    const denied = await page.evaluate(async () => {
      let fetchDenied = false;
      try { await fetch('/_watch/boards'); } catch { fetchDenied = true; }
      const scriptDenied = await new Promise(resolve => {
        const script = document.createElement('script'); script.type = 'module'; script.src = '/static/thread-watcher.v1.js';
        script.onload = () => resolve(false); script.onerror = () => resolve(true); document.body.append(script);
      });
      return { fetchDenied, scriptDenied };
    });
    expect(denied).toEqual({ fetchDenied: true, scriptDenied: true });
    await expect(page.locator('[data-native-settings-ready], #threadWatcher')).toHaveCount(0);
    await page.locator('#boardSelectMobile').selectOption('a'); await expect(page).toHaveURL(`${origin}/a/`);
    await expect(page.locator('.boardTitle')).toContainText('/a/');
    expect(errors).toEqual([]);
  } finally { await context.close(); archiveFixture('cleanup', slug); }
});
