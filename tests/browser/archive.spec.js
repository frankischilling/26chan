import { test, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import path from 'node:path';

function fixture(command, slug) {
  const binary = process.platform === 'win32' ? '.exe' : '';
  const result = spawnSync(path.resolve(`target/debug/examples/archive-fixture${binary}`), [command, slug], {
    encoding: 'utf8', timeout: 15_000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot },
  });
  expect(result.status, 'Owned archive fixture helper must succeed').toBe(0);
}

test('archive navigation, read-only threads, reports and deletion work without JavaScript', async ({ browser }) => {
  const slug = `z${randomBytes(5).toString('hex').slice(0, 9)}`;
  const origin = 'http://127.0.0.1:3000';
  const password = 'archive-browser-password';
  fixture('setup', slug);
  let context;
  try {
    context = await browser.newContext({ javaScriptEnabled: false });
    const page = await context.newPage();
    await page.goto(`${origin}/${slug}/`);
    await page.getByRole('link', { name: 'Archive', exact: true }).click();
    await expect(page.getByText('No archived threads.', { exact: true })).toBeVisible();
    await page.getByRole('link', { name: 'Index', exact: true }).click();
    await page.locator('#sub').fill('<b>Synthetic archive subject</b>');
    await page.locator('#com').fill('Owned first thread displaced by a second thread.');
    await page.locator('#password').fill(password);
    await page.getByRole('button', { name: 'Post', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/${slug}/thread/\\d+#p\\d+$`));
    const archived = /#p(\d+)$/.exec(page.url())[1];
    await page.getByRole('link', { name: 'Index', exact: true }).click();
    await page.locator('#com').fill('Owned replacement thread triggers rollover.');
    await page.locator('#password').fill(password);
    await page.getByRole('button', { name: 'Post', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/${slug}/thread/\\d+#p\\d+$`));
    await page.getByRole('link', { name: 'Archive', exact: true }).click();
    const summary = page.getByRole('link', { name: `No.${archived} — <b>Synthetic archive subject</b>`, exact: true });
    await expect(summary).toBeVisible();
    await expect(summary.locator('b')).toHaveCount(0);
    await expect(page.locator('.archiveEntries time')).toBeVisible();
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
    await page.locator(`#delete${archived}`).fill(password);
    await page.getByRole('button', { name: 'Delete post', exact: true }).click();
    await expect(page).toHaveURL(`${origin}/${slug}/`);
    await page.getByRole('link', { name: 'Archive', exact: true }).click();
    await expect(page.getByText('No archived threads.', { exact: true })).toBeVisible();
    expect((await context.request.get(`${origin}/${slug}/thread/${archived}.json`)).status()).toBe(404);
  } finally {
    try { if (context) await context.close(); }
    finally { fixture('cleanup', slug); }
  }
});
