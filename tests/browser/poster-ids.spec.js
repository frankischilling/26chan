import { withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';
import { randomBytes } from 'node:crypto';
import { execFileSync } from 'node:child_process';

function sql(board, input) {
  const database = new URL(process.env.MIGRATION_DATABASE_URL);
  // This fixture owns rows only on the disposable loopback development cluster.
  expect(['127.0.0.1', 'localhost']).toContain(database.hostname);
  expect(database.pathname).toBe('/imageboard');
  const env = { ...process.env, PGHOST: database.hostname, PGPORT: database.port || '5432',
    PGDATABASE: 'imageboard', PGUSER: decodeURIComponent(database.username),
    PGPASSWORD: decodeURIComponent(database.password), PGSSLMODE: 'disable' };
  return execFileSync('psql', ['-Xq', '-v', 'ON_ERROR_STOP=1', '-v', `board=${board}`],
    { input, env, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] });
}

for (const sage of [false, true]) {
  test(`persisted ${sage ? 'sage' : 'network'} IDs render, color, filter and update through actual public projections`, async ({ page, request }) => {
    const board = `id${randomBytes(4).toString('hex')}`, origin = 'http://127.0.0.1:3000';
    sql(board, `INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,user_ids)
      VALUES(:'board','Owned browser IDs','Synthetic fixture',1000,100,100,100,10,true);`);
    const post = async (parent, com) => {
      const response = await withPostingHistory(() => request.post(`/${board}/post`, { headers: { Origin: origin }, maxRedirects: 0,
        form: { resto: parent, name: 'Owned', email: sage ? 'sage' : '', sub: 'Owned poster IDs', com, password: 'owned-poster-password' } }));
      expect(response.status()).toBe(303);
      return response.headers().location.match(/#p(\d+)$/)[1];
    };
    try {
      const thread = await post('0', 'Owned opening post');
      const reply = await post(thread, 'Owned reply');
      const remote = await post('0', `>>${thread}\nOwned remote quote`);
      const data = await (await request.get(`/${board}/thread/${thread}.json`)).json();
      expect(data.posts[0].id).toMatch(/^[+/0-9A-Za-z]{8}$/);
      const id = sage ? 'Heaven' : data.posts[0].id;
      expect(data.posts[1].id).toBe(id);
      const other = await (await request.get(`/${board}/thread/${remote}.json`)).json();
      expect(other.posts[0].id).not.toBe(data.posts[0].id);
      await page.addInitScript(() => {
        if (localStorage.getItem('4chan-settings') === null) {
          localStorage.setItem('4chan-settings', JSON.stringify({ quotePreview: true, filter: true, threadStats: false }));
        }
      });
      await page.goto(`/${board}/thread/${remote}`);
      await expect(page.locator(`#m${remote} .quotelink`)).toHaveAttribute('href', `/${board}/thread/${thread}#p${thread}`);
      await page.locator(`#m${remote} .quotelink`).hover();
      await expect(page.locator('#quote-preview .postInfo .posteruid .hand')).toHaveText(id);
      expect(await page.locator('#quote-preview .postInfo .posteruid .hand').evaluate(element => element.style.backgroundColor)).toMatch(/^rgb\(/);
      await page.goto(`/${board}/thread/${thread}`);
      await expect(page.locator(`#pi${reply} .posteruid .hand`)).toHaveText(id);
      for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 900 });
        expect(await page.locator(`#p${reply} .posteruid .hand:visible`).evaluate(element => getComputedStyle(element).backgroundColor)).toMatch(/^rgb\(/);
      }
      const label = page.locator(`#p${thread} .posteruid .hand:visible`);
      await label.click();
      await expect(label).toHaveAttribute('aria-pressed', 'true');
      await expect(page.locator(`#p${reply}`)).toHaveClass(/poster-id-highlight/);
      await label.focus();
      await expect(page.locator('#native-poster-id-tip')).toHaveText('2 posts by this ID');
      const added = await post(thread, 'Owned live reply');
      await page.locator('.threadNav.mobile a[data-cmd="update"]').first().click();
      await expect(page.locator(`#p${added} .posteruid .hand:visible`)).toHaveText(id);
      await expect(page.locator(`#p${added} .posteruid .hand:visible`)).toHaveCSS('border-radius', '6px');
      await expect(page.locator(`#p${added}`)).toHaveClass(/poster-id-highlight/);
      await label.focus();
      await expect(page.locator('#native-poster-id-tip')).toHaveText('3 posts by this ID');
      await label.press('Enter');
      await expect(page.locator(`#p${reply}`)).not.toHaveClass(/poster-id-highlight/);
      await page.evaluate(id => {
        localStorage.setItem('4chan-settings', JSON.stringify({ filter: true, threadStats: false, IDColor: false }));
        localStorage.setItem('4chan-filters', JSON.stringify([{ type: 4, pattern: id, boards: '', active: true, auto: false, hide: true }]));
      }, id);
      await page.reload();
      await expect(page.locator(`#p${reply}`)).toHaveClass(/post-hidden/);
      await expect(page.locator(`#p${added}`)).toHaveClass(/post-hidden/);
      expect(await page.locator(`#p${thread} .posteruid .hand:visible`).evaluate(element => element.style.backgroundColor)).toBe('');
      await expect(page.locator(`#p${thread} .posteruid .hand:visible`)).toBeVisible();
    } finally {
      sql(board, `DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=:'board');
        DELETE FROM content.posts WHERE board=:'board'; DELETE FROM content.threads WHERE board=:'board';
        DELETE FROM content.boards WHERE slug=:'board';`);
    }
  });
}
