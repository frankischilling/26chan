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
