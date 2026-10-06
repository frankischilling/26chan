import { withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import path from 'node:path';

function fixture(command, slug) {
  const binary = process.platform === 'win32' ? '.exe' : '';
  const result = spawnSync(path.resolve(process.env.CARGO_TARGET_DIR || 'target', `debug/examples/archive-fixture${binary}`), [command, slug], {
    encoding: 'utf8', timeout: 15_000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot },
  });
  expect(result.error, 'Owned archive fixture helper must launch').toBeUndefined();
  expect(result.status, 'Owned archive fixture helper must succeed').toBe(0);
}

test('archive navigation and reports work without JavaScript while public deletion stays forbidden', async ({ browser }) => {
  const slug = `z${randomBytes(5).toString('hex').slice(0, 9)}`;
  const origin = 'http://127.0.0.1:3000';
  fixture('setup', slug);
  let context;
  try {
    context = await browser.newContext({ javaScriptEnabled: false });
    const page = await context.newPage();
    await page.goto(`${origin}/${slug}/`);
    await page.getByRole('link', { name: 'Archive', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Displaying 0 expired threads from the past 3 days', exact: true })).toBeVisible();
    await expect(page.locator('#arc-list tbody tr')).toHaveCount(0);
    await expect(page.locator('#arc-list thead td')).toHaveText(['No.', 'Excerpt', '']);
    await page.getByRole('link', { name: 'Index', exact: true }).click();
    await page.locator('#sub').fill('<b>Synthetic archive subject</b>');
    await page.locator('#com').fill('Owned first thread displaced by a second thread.');
    await expect(page.locator('#postPassword')).toHaveValue('');
    await withPostingHistory(() => page.getByRole('button', { name: 'Post', exact: true }).click());
    await expect(page).toHaveURL(new RegExp(`/${slug}/thread/\\d+#p\\d+$`));
    const archived = /#p(\d+)$/.exec(page.url())[1];
    await page.getByRole('link', { name: 'Return', exact: true }).first().click();
    await expect(page).toHaveURL(`${origin}/${slug}/`);
    await page.locator('#com').fill('Owned replacement thread triggers rollover.');
    await expect(page.locator('#postPassword')).toHaveValue('');
    await withPostingHistory(() => page.getByRole('button', { name: 'Post', exact: true }).click());
    await expect(page).toHaveURL(new RegExp(`/${slug}/thread/\\d+#p\\d+$`));
    await page.getByRole('link', { name: 'Return', exact: true }).first().click();
    await expect(page).toHaveURL(`${origin}/${slug}/`);
    await page.getByRole('link', { name: 'Archive', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Displaying 1 expired thread from the past 3 days', exact: true })).toBeVisible();
    const row = page.locator('#arc-list tbody tr');
    await expect(row).toHaveCount(1);
    await expect(row.locator('td').first()).toHaveText(archived);
    await expect(row.locator('.teaser-col')).toContainText('<b>Synthetic archive subject</b>');
    // Serialized subject markup pushes this fixture past the source's 100-character
    // cutoff, so the excerpt strips generated bold as well as preserving literal tags.
    await expect(row.locator('.teaser-col b')).toHaveCount(0);
    await expect(row.locator('.teaser-col')).toContainText('Owned first thread displaced');
    const summary = row.getByRole('link', { name: 'View', exact: true });
    await expect(summary).toBeVisible();
    await expect(summary).toHaveAttribute('href', new RegExp(`^/${slug}/thread/${archived}(?:/[a-z0-9-]+)?$`));
    await expect(page.locator('#arc-list time')).toHaveCount(0);
    expect(await (await context.request.get(`${origin}/${slug}/archive.json`)).json()).toEqual([Number(archived)]);
    await summary.click();
    await expect(page.getByText('This thread is archived and read-only.', { exact: true })).toBeVisible();
    await expect(page.locator('#postForm')).toHaveCount(0);
    await page.locator(`#p${archived} summary`).click();
    await page.locator(`#report${archived}`).fill('Owned archived post report');
    await page.getByRole('button', { name: 'Report post', exact: true }).click();
    await expect(page.getByRole('heading', { name: 'Report received' })).toBeVisible();
    await page.goto(`${origin}/${slug}/thread/${archived}`);
    await page.locator(`#p${archived} summary`).click();
    await expect(page.locator(`#delete${archived}`)).toHaveValue('');
    const before = await (await context.request.get(`${origin}/${slug}/thread/${archived}.json`)).json();
    const denied = page.waitForResponse(response => response.request().method() === 'POST' && response.url().endsWith(`/${slug}/delete`));
    await page.getByRole('button', { name: 'Delete post', exact: true }).click();
    expect((await denied).status()).toBe(403);
    await expect(page.locator('body')).toContainText('Error: Password incorrect.');
    expect(await (await context.request.get(`${origin}/${slug}/thread/${archived}.json`)).json()).toEqual(before);
    expect(await (await context.request.get(`${origin}/${slug}/archive.json`)).json()).toEqual([Number(archived)]);
    fixture('expire', slug);
    await page.goto(`${origin}/${slug}/archive`);
    await expect(page.getByRole('heading', { name: 'Displaying 0 expired threads from the past 3 days', exact: true })).toBeVisible();
    await expect(page.locator('#arc-list tbody tr')).toHaveCount(0);
    await expect(page.locator('#arc-list thead td')).toHaveText(['No.', 'Excerpt', '']);
    expect((await context.request.get(`${origin}/${slug}/thread/${archived}.json`)).status()).toBe(404);
  } finally {
    try { if (context) await context.close(); }
    finally { fixture('cleanup', slug); }
  }
});

for (const javaScriptEnabled of [false, true]) {
  test(`legacy res lookup redirects live and retained archived OPs and replies with JavaScript ${javaScriptEnabled ? 'enabled' : 'disabled'}`, async ({ browser, request }) => {
    const slug = `z${randomBytes(5).toString('hex').slice(0, 9)}`;
    const origin = 'http://127.0.0.1:3000';
    let context;
    fixture('setup', slug);
    try {
      // Posting uses a separate request context; lookup readers start without
      // an anonymous session or posting receipt that could conceal minting.
      const post = async (parent, comment) => {
        const response = await withPostingHistory(() => request.post(`${origin}/${slug}/post`, {
          headers: { Origin: origin }, maxRedirects: 0,
          form: { resto: parent, sub: parent === '0' ? 'Owned res lookup' : '', com: comment, password: 'owned-res-lookup-password' },
        }));
        expect(response.status()).toBe(303);
        return /#p(\d+)$/.exec(response.headers().location)[1];
      };
      const op = await post('0', 'Owned lookup OP');
      const reply = await post(op, 'Owned lookup reply');
      for (const archived of [false, true]) {
        if (archived) {
          await post('0', 'Owned replacement triggers real archive rollover');
          expect(await (await request.get(`${origin}/${slug}/archive.json`)).json()).toEqual([Number(op)]);
        }
        // Fresh contexts also prevent a cached permanent redirect from hiding
        // the real server lookup after the target moves into the archive.
        context = await browser.newContext({ javaScriptEnabled });
        const page = await context.newPage();
        const writes = [], completionMessages = [];
        context.on('request', request => { if (request.method() === 'POST') writes.push(request.url()); });
        await context.exposeBinding('observeLookupCompletion', (_, message) => completionMessages.push(message));
        await context.addInitScript(() => window.addEventListener('message', event => {
          if (typeof event.data === 'string' && event.data.startsWith('done-report')) {
            window.observeLookupCompletion(event.data);
          }
        }));
        expect(await context.cookies()).toEqual([]);
        for (const target of [op, reply]) {
          const lookup = `${origin}/${slug}/imgboard.php?res=${target}`;
          const destination = `${origin}/${slug}/thread/${op}#p${target}`;
          const loaded = await page.goto(lookup);
          expect(loaded.status()).toBe(200);
          const redirected = loaded.request().redirectedFrom();
          expect(redirected).not.toBe(null);
          expect(redirected.url()).toBe(lookup);
          expect(redirected.method()).toBe('GET');
          const redirect = await redirected.response();
          expect(redirect.status()).toBe(301);
          expect(redirect.headers().location).toBe(`/${slug}/thread/${op}#p${target}`);
          expect(await redirect.headerValue('set-cookie')).toBe(null);
          expect(await loaded.headerValue('set-cookie')).toBe(null);
          await expect(page).toHaveURL(destination);
          await expect(page.locator(`#p${target}`)).toBeVisible();
          await expect(page.getByText('This thread is archived and read-only.', { exact: true })).toHaveCount(archived ? 1 : 0);
          await expect(page.locator('#report-popup-context, script[src="/static/report-popup.v1.js"]')).toHaveCount(0);
          expect(await context.cookies()).toEqual([]);
          expect(writes).toEqual([]);
          expect(completionMessages).toEqual([]);
        }
        await context.close();
        context = null;
      }
    } finally {
      try { if (context) await context.close(); }
      finally { fixture('cleanup', slug); }
    }
  });
}

for (const javaScriptEnabled of [false, true]) {
  test(`bare legacy index GET uses its native two-second refresh with JavaScript ${javaScriptEnabled ? 'enabled' : 'disabled'}`, async ({ browser }) => {
    const slug = `z${randomBytes(5).toString('hex').slice(0, 9)}`;
    const origin = 'http://127.0.0.1:3000';
    let context;
    fixture('setup', slug);
    try {
      context = await browser.newContext({ javaScriptEnabled });
      const page = await context.newPage();
      const writes = [], popups = [], completionMessages = [];
      context.on('request', request => { if (request.method() === 'POST') writes.push(request.url()); });
      context.on('page', popup => popups.push(popup));
      await context.exposeBinding('observeIndexCompletion', (_, message) => completionMessages.push(message));
      await context.addInitScript(() => window.addEventListener('message', event => {
        if (typeof event.data === 'string' && event.data.startsWith('done-report')) {
          window.observeIndexCompletion(event.data);
        }
      }));
      expect(await context.cookies()).toEqual([]);
      const bare = `${origin}/${slug}/imgboard.php`, index = `${origin}/${slug}/`;
      const refreshed = page.waitForResponse(response => response.url() === index
        && response.request().isNavigationRequest() && response.request().method() === 'GET');
      // Inspect the actual initial response, not a transient DOM that may
      // already have refreshed on a slow runner. Native refresh is not a JS timer.
      const initial = await page.goto(bare, { waitUntil: 'commit' });
      expect(initial.status()).toBe(200);
      expect(initial.url()).toBe(bare);
      expect(initial.request().redirectedFrom()).toBe(null);
      expect(await initial.headerValue('location')).toBe(null);
      expect(await initial.headerValue('set-cookie')).toBe(null);
      const html = await initial.text();
      expect(html).toContain('<strong>Updating index...</strong>');
      const refresh = /<meta\b(?=[^>]*\bhttp-equiv=["']refresh["'])[^>]*\bcontent=["']([^"']+)["'][^>]*>/i.exec(html);
      expect(refresh, 'Initial response must contain a native meta refresh').not.toBe(null);
      expect(refresh[1]).toMatch(new RegExp(`^2;\\s*URL=/${slug}/$`, 'i'));
      expect(html).not.toContain('report-popup-context');
      expect(html).not.toContain('/static/report-popup.v1.js');
      const loaded = await refreshed;
      expect(loaded.status()).toBe(200);
      // Meta refresh starts a distinct navigation, not an HTTP redirect chain.
      expect(loaded.request().redirectedFrom()).toBe(null);
      expect(await loaded.headerValue('set-cookie')).toBe(null);
      await expect(page).toHaveURL(index);
      await expect(page.getByRole('heading', { name: `/${slug}/ - Synthetic archive browser test`, exact: true })).toBeVisible();
      expect(await context.cookies()).toEqual([]);
      expect(writes).toEqual([]);
      expect(popups).toEqual([]);
      expect(completionMessages).toEqual([]);
    } finally {
      try { if (context) await context.close(); }
      finally { fixture('cleanup', slug); }
    }
  });
}
