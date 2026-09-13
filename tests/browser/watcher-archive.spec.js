import { test, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import path from 'node:path';

const origin = 'http://127.0.0.1:3000';
function fixture(command, slug) {
  const suffix = process.platform === 'win32' ? '.exe' : '';
  const result = spawnSync(path.resolve(`target/debug/examples/archive-fixture${suffix}`), [command, slug], {
    encoding: 'utf8', timeout: 15_000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot },
  });
  expect(result.status, 'Owned archive fixture helper must succeed').toBe(0);
}

test('real rollover preserves watched unread state and expiry becomes dead before pruning', async ({ page, request }) => {
  const slug = `z${randomBytes(5).toString('hex').slice(0, 9)}`;
  fixture('setup', slug);
  try {
    const post = async (resto, subject, comment) => {
      const response = await request.post(`/${slug}/post`, { headers: { Origin: origin },
        form: { resto, sub: subject, com: comment, password: 'owned-watch-archive-password' }, maxRedirects: 0 });
      expect(response.status()).toBe(303);
      return response.headers().location.match(resto === '0' ? /thread\/(\d+)/ : /#p(\d+)/)[1];
    };
    const id = await post('0', 'Archived watch', 'Owned watched opening post');
    const key = `${id}-${slug}`;
    const endpoint = `/_watch/${slug}/thread/${id}.json`;
    const tuple = () => page.evaluate(key => JSON.parse(localStorage.getItem('4chan-watch'))[key] ?? null, key);
    const refresh = async () => {
      // Catalog initialization with no timestamp leaves manual Refresh eligible.
      await page.evaluate(() => localStorage.removeItem('4chan-tw-timestamp'));
      await page.reload();
      await page.locator('#twPrune').click();
      await expect(page.locator('.watcherNotice')).toHaveText('Refresh complete.');
      await expect(page.locator('#threadWatcher')).toHaveAttribute('aria-busy', 'false');
    };
    await page.goto(`/${slug}/catalog`);
    await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true })));
    await page.reload();
    await page.getByRole('button', { name: `Watch thread ${id}`, exact: true }).click();
    await expect(page.locator(`#watch-${key}`)).toBeVisible();
    const reply = await post(id, '', 'Owned unread reply before rollover');
    await refresh();
    expect((await tuple())[2]).toBe(1);

    await post('0', 'Replacement watch fixture', 'This real post rolls over the one-thread board.');
    const archived = await request.get(endpoint);
    expect(archived.status()).toBe(200);
    const archivedPosts = (await archived.json()).posts;
    expect(archivedPosts[0].archived).toBe(1);
    expect(archivedPosts[0].closed).toBe(1);
    expect(archivedPosts.map(post => String(post.no))).toEqual([id, reply]);
    expect((await request.get(`/${slug}/catalog.json`)).status()).toBe(200);
    expect((await (await request.get(`/${slug}/archive.json`)).json()).map(String)).toEqual([id]);
    await refresh();
    const link = page.locator(`#watch-${key} a`);
    await expect(link).toHaveText(`(1) /${slug}/ - Archived watch`);
    await expect(link).toHaveClass(/hasNewReplies/);
    await expect(link).toHaveClass(/archivelink/);
    expect((await tuple())[3]).toBe(1);
    await link.click();
    await expect(page.getByText('This thread is archived and read-only.', { exact: true })).toBeVisible();
    await expect(page.locator('#postForm')).toHaveCount(0);
    await expect.poll(async () => (await tuple())[2]).toBe(0);
    expect(String((await tuple())[1])).toBe(reply);
    expect((await tuple())[3]).toBe(1);

    // Only this helper-created board's single archived thread is expired.
    fixture('expire', slug);
    expect((await request.get(endpoint)).status()).toBe(404);
    expect(await (await request.get(`/${slug}/archive.json`)).json()).toEqual([]);
    await page.goto(`/${slug}/catalog`);
    await refresh();
    expect((await tuple())[1]).toBe(-1);
    await expect(link).toHaveClass('deadlink');
    let reads = 0;
    page.on('request', request => { if (request.url().endsWith(endpoint)) reads++; });
    await refresh();
    expect(await tuple()).toBeNull();
    await expect(page.locator(`#watch-${key}`)).toHaveCount(0);
    expect(reads).toBe(0);
  } finally { fixture('cleanup', slug); }
});
