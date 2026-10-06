import { withDeletionQuota } from './helpers/deletion-quota-fixture.js';
import { ownedDeletionMarker, deletionFixture } from './helpers/deletion-fixture.js';
import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';

const origin = 'http://127.0.0.1:3000', password = 'owned-identity-browser-password';
const encoding = JSON.parse(readFileSync(new URL('../../crates/domain/tests/fixtures/trip-encoding.json', import.meta.url), 'utf8'));

test('source boards suppress both trip types in native and Quick Reply posts', async ({ browser, request }) => {
  for (const board of ['b', 's4s']) {
    for (const width of [1280, 390]) {
      const marker = ownedDeletionMarker();
      const created = await request.post(`/${board}/post`, { headers: { Origin: origin }, maxRedirects: 0,
        form: { name: '#password', sub: marker, com: 'Owned source suppression thread', password } });
      expect(created.status()).toBe(303);
      const thread = /#p(\d+)$/.exec(created.headers().location)[1];
      const context = await browser.newContext({ viewport: { width, height: 900 }, isMobile: width === 390,
        hasTouch: width === 390, javaScriptEnabled: width === 390 });
      try {
        if (width === 390) {
          await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: true, threadStats: false })));
        }
        const page = await context.newPage();
        await page.goto(`/${board}/thread/${thread}?quote=${thread}`);
        const info = page.locator(`#${width === 390 ? 'pim' : 'pi'}${thread}`);
        await expect(info.locator('.name')).toHaveText('Anonymous');
        await expect(page.locator('.postertrip')).toHaveCount(0);
        if (width === 390) {
          await info.locator('a[title="Reply to this post"]').click();
          await expect(page.locator('#quickReply')).toBeVisible();
          await page.locator('#qr-name').fill('Named##password');
          await page.locator('#qrCom').fill('Owned suppressed secure reply');
          await page.locator('#quickReply input[type=submit]').click();
          await expect(page.locator('.postMessage').filter({ hasText: 'Owned suppressed secure reply' })).toHaveCount(1);
        } else {
          const form = page.locator('form[name=post]');
          await form.locator('[name=name]').fill('Named##password');
          await form.locator('[name=com]').fill('Owned suppressed secure reply');
          await form.locator('button[type=submit]').click();
          await expect(page).toHaveURL(new RegExp(`/thread/${thread}#p[0-9]+$`));
        }
        const data = await (await request.get(`/${board}/thread/${thread}.json`)).json();
        expect(data.posts).toHaveLength(2);
        expect(data.posts.map(post => post.name)).toEqual(['Anonymous', 'Named']);
        expect(data.posts.every(post => !Object.hasOwn(post, 'trip'))).toBe(true);
        await expect(page.locator(`#pi${data.posts[1].no} .name`)).toHaveText('Named');
        await expect(page.locator('.postertrip')).toHaveCount(0);
      } finally {
        try { await context.close(); } finally { deletionFixture('cleanup', board, thread, marker); }
      }
    }
  }
});

test('persisted tripcodes survive browser previews, filters and ordinary rendering', async ({ page, context, request }) => {
  const threads = [];
  const post = async (parent, name, com) => {
    const response = await request.post('/demo/post', { headers: { Origin: origin }, maxRedirects: 0,
      form: { resto: parent, name, sub: 'Owned identity', com, password } });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/#p(\d+)$/)[1];
    if (parent === '0') threads.push(id);
    return id;
  };
  try {
    const target = await post('0', '<owned name>#password', 'Owned tripcode target');
    const reply = await post(target, 'Reply#password', 'Owned reply with the same pseudonym');
    const remote = await post('0', 'Plain name', `>>${target}\nOwned remote reference`);
    await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quotePreview: true, threadStats: false, filter: true })));
    await page.goto(`/demo/thread/${remote}`);
    await page.locator(`#m${remote} a.quotelink`).hover();
    const preview = page.locator('#quote-preview');
    await expect(preview).toBeVisible();
    await expect(preview.locator('.postInfo .postertrip')).toHaveText('!ozOtJW9BFA');
    await expect(preview.locator('.postInfo .name')).toHaveText('<owned name>');
    await expect(preview.locator('owned, script, form, input')).toHaveCount(0);
    await page.goto(`/demo/thread/${target}`);
    await expect(page.locator(`#pi${target} .postertrip`)).toHaveText('!ozOtJW9BFA');
    await expect(page.locator(`#pi${reply} .postertrip`)).toHaveText('!ozOtJW9BFA');
    for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
      await context.addCookies([{ name: 'board-theme-ws', value: theme, url: origin, httpOnly: true, sameSite: 'Lax' }]);
      await page.reload();
      const style = await page.locator(`#pi${reply} .postertrip`).evaluate(node => {
        const actual = getComputedStyle(node), name = getComputedStyle(node.parentElement.querySelector('.name'));
        return { weight: actual.fontWeight, sameColor: actual.color === name.color };
      });
      expect(style, theme).toEqual({ weight: '400', sameColor: true });
    }
    await page.evaluate(() => localStorage.setItem('4chan-filters', JSON.stringify([
      { type: 0, pattern: '!ozOtJW9BFA', boards: '', active: true, auto: false, hide: true },
    ])));
    await page.reload();
    await expect(page.locator(`#p${reply}`)).toHaveClass(/post-hidden/);
    await expect(page.locator(`#m${reply}`)).toBeHidden();
    await expect(page.locator(`#pi${target} .postertrip`)).toBeVisible();
    const data = await (await request.get(`/demo/thread/${target}.json`)).json();
    expect(data.posts.map(post => post.trip)).toEqual(['!ozOtJW9BFA', '!ozOtJW9BFA']);
    expect(data.posts.map(post => post.name)).toEqual(['&lt;owned name&gt;', 'Reply']);
  } finally {
    for (const thread of threads.reverse()) {
      await withDeletionQuota(async () => {
        expect((await request.post('/demo/delete', { headers: { Origin: origin }, maxRedirects: 0,
          form: { no: thread, password } })).status()).toBe(303);
      });
    }
  }
});

test('CP932 trip-only names and cleaned text survive desktop, mobile and live rendering', async ({ browser, request }) => {
  const trip = `!${encoding.vectors.find(vector => vector.input === 'かみ').trip}`;
  for (const width of [1280, 390]) {
    const created = await request.post('/demo/post', { headers: { Origin: origin }, maxRedirects: 0,
      form: { name: '#かみ', com: 'Owned trip-only source thread', password } });
    expect(created.status()).toBe(303);
    const thread = /#p(\d+)$/.exec(created.headers().location)[1];
    const context = await browser.newContext({ viewport: { width, height: 900 }, isMobile: width === 390, hasTouch: width === 390 });
    try {
      await context.addInitScript(() => {
        localStorage.setItem('4chan-settings', JSON.stringify({ filter: true, quickReply: true, quotePreview: true, threadStats: false, threadWatcher: false }));
        localStorage.setItem('4chan-filters', JSON.stringify([{ type: 1, pattern: 'Anonymous', boards: '', active: true, auto: true, hide: true }]));
      });
      const page = await context.newPage();
      await page.goto(`/demo/catalog`);
      const card = page.locator(`a.catalogThumb[href="/demo/thread/${thread}"]`);
      await expect(card).toHaveAttribute('data-filter-name', '');
      await expect(card).toBeVisible();
      await page.goto(`/demo/thread/${thread}`);
      const info = page.locator(`#${width === 390 ? 'pim' : 'pi'}${thread}`);
      await expect(info.locator('.name')).toHaveText('');
      await expect(info.locator('.postertrip')).toHaveText(trip);
      let data = await (await request.get(`/demo/thread/${thread}.json`)).json();
      expect(Object.hasOwn(data.posts[0], 'name')).toBe(false);
      expect(data.posts[0].trip).toBe(trip);
      await info.locator('a[title="Reply to this post"]').click();
      await expect(page.locator('#quickReply')).toBeVisible();
      await page.locator('#qr-name').fill('＃Named！<owned>&"\'');
      await page.locator('#qrCom').fill('Owned cleaned name result');
      await page.locator('#quickReply input[type=submit]').click();
      await expect(page.locator('.postMessage').filter({ hasText: 'Owned cleaned name result' })).toHaveCount(1);
      data = await (await request.get(`/demo/thread/${thread}.json`)).json();
      expect(data.posts).toHaveLength(2);
      expect(data.posts[1].name).toBe('Named&lt;owned&gt;&amp;&quot;&#039;');
      expect(data.posts[1].trip).toBeUndefined();
      const reply = data.posts[1].no;
      await expect(page.locator(`#pi${reply} .name`)).toHaveText('Named<owned>&"\'');
      await expect(page.locator('owned')).toHaveCount(0);
    } finally {
      await context.close();
      await withDeletionQuota(async () => {
        expect((await request.post('/demo/delete', { headers: { Origin: origin }, maxRedirects: 0,
          form: { no: thread, password } })).status()).toBe(303);
      });
    }
  }
});
