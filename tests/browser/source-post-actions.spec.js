import { test as base, expect } from '@playwright/test';
import { ownedDeletionMarker } from './helpers/deletion-fixture.js';
import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';

const origin = 'http://127.0.0.1:3000';
const password = 'owned-source-batch-password';
const headers = { Origin: origin, Connection: 'close' };
const test = base.extend({
  owned: async ({ context, request }, use) => {
    const marker = ownedDeletionMarker();
    let thread;
    async function write(parent, comment, writer = context) {
      const response = await withPostingHistory(() => writer.request.post('/fixture/post', {
        headers, maxRedirects: 0,
        form: { resto: parent, sub: parent === '0' ? marker : '', com: comment, password },
      }));
      expect(response.status()).toBe(303);
      const receipt = /\/fixture\/thread\/([1-9][0-9]*)#p([1-9][0-9]*)$/.exec(response.headers().location);
      expect(receipt).not.toBeNull();
      expect(receipt[1]).toBe(parent === '0' ? receipt[2] : parent);
      return receipt[2];
    }
    try {
      thread = await write('0', 'Owned batch OP');
      await use({ thread, write, url: `/fixture/thread/${thread}` });
    } finally {
      if (thread && (await request.get(`/fixture/thread/${thread}.json`)).status() === 200) {
        await withDeletionQuota(async () => {
          const response = await request.post('/fixture/delete', { headers, maxRedirects: 0, form: { no: thread, password } });
          expect(response.status()).toBe(303);
          expect((await request.get(`/fixture/thread/${thread}.json`)).status()).toBe(404);
        });
      }
    }
  },
});

test.use({ viewport: { width: 1280, height: 900 }, trace: 'off' });
async function postIds(request, owned) {
  const response = await request.get(`${owned.url}.json`);
  expect(response.status()).toBe(200);
  return (await response.json()).posts.map(post => String(post.no));
}
async function submit(page, status) {
  await withDeletionQuota(async () => {
    const response = page.waitForResponse(response => response.request().method() === 'POST'
      && response.url() === `${origin}/fixture/imgboard.php`);
    await page.locator('#delform input[type="submit"]').click();
    expect((await response).status()).toBe(status);
    await page.waitForLoadState('domcontentloaded');
  });
}

for (const javaScriptEnabled of [false, true]) {
  test.describe(`source batch controls with JavaScript ${javaScriptEnabled ? 'enabled' : 'disabled'}`, () => {
    test.use({ javaScriptEnabled });

    test('real native multi-selection removes only the chosen replies', async ({ page, request, owned }) => {
      const first = await owned.write(owned.thread, 'Owned first batch reply');
      const second = await owned.write(owned.thread, 'Owned second batch reply');
      const keep = await owned.write(owned.thread, 'Owned unselected control');
      await page.goto(owned.url);
      const forms = await page.locator('#delform').evaluate(form => ({ action: form.getAttribute('action'), nested: !!form.querySelector('form') }));
      expect(forms).toEqual({ action: '/fixture/imgboard.php', nested: false });
      await page.getByRole('checkbox', { name: `Select post No.${first}`, exact: true }).check();
      await page.getByRole('checkbox', { name: `Select post No.${second}`, exact: true }).check();
      const fields = await page.locator('#delform').evaluate(form => [...new FormData(form)]);
      expect(fields).toEqual([[first, 'delete'], [second, 'delete'], ['mode', 'usrdel'], ['res', owned.thread], ['pwd', '']]);
      await submit(page, 301);
      await expect(page).toHaveURL(`${origin}${owned.url}#p${owned.thread}`);
      expect(await postIds(request, owned)).toEqual([owned.thread, keep]);
    });

    test('file-only native batch preserves text-only posts and their public bodies', async ({ page, request, owned }) => {
      const reply = await owned.write(owned.thread, 'Owned file-only text remains');
      await page.goto(owned.url);
      await page.getByRole('checkbox', { name: `Select post No.${reply}`, exact: true }).check();
      await page.locator('#delform [name="onlyimgdel"]').check();
      await submit(page, 301);
      expect(await postIds(request, owned)).toEqual([owned.thread, reply]);
      const response = await request.get(`${owned.url}.json`);
      expect((await response.json()).posts[1].com).toContain('Owned file-only text remains');
    });

    for (const fromIndex of [false, true]) {
      test(`deleting the opening post from ${fromIndex ? 'index' : 'thread'} returns through the source updating-index page`, async ({ page, request, owned }) => {
        await page.goto(fromIndex ? '/fixture/' : owned.url);
        await expect(page.locator('#delform [name="res"]')).toHaveCount(fromIndex ? 0 : 1);
        await page.getByRole('checkbox', { name: `Select post No.${owned.thread}`, exact: true }).check();
        await submit(page, 200);
        await expect(page).toHaveURL(`${origin}/fixture/`);
        expect((await request.get(`${owned.url}.json`)).status()).toBe(404);
      });
    }

    test('a later ownership refusal preserves the earlier committed selection', async ({ page, request, browser, owned }) => {
      const first = await owned.write(owned.thread, 'Owned earlier successful selection');
      const stranger = await browser.newContext({ baseURL: origin });
      try {
        const foreign = await owned.write(owned.thread, 'Owned other-session control', stranger);
        await page.goto(owned.url);
        await page.getByRole('checkbox', { name: `Select post No.${first}`, exact: true }).check();
        await page.getByRole('checkbox', { name: `Select post No.${foreign}`, exact: true }).check();
        await submit(page, 403);
        expect(await postIds(request, owned)).toEqual([owned.thread, foreign]);
      } finally { await stranger.close(); }
    });
  });
}
