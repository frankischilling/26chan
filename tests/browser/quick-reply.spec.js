import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000', password = 'owned-quick-reply-password';

test('post permalinks stay navigable while digits quote original and live-updated posts', async ({ page, request }) => {
  const created = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0,
    form: { com: 'Owned post-number thread', password } }));
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  try {
    await page.goto(`/fixture/thread/${id}`);
    const numbers = page.locator(`#pi${id} > .postNum`);
    await expect(numbers.locator('a')).toHaveCount(2);
    await expect(numbers.getByTitle('Link to this post', { exact: true })).toHaveText('No.');
    await numbers.getByTitle('Link to this post', { exact: true }).click();
    await expect(page).toHaveURL(`${origin}/fixture/thread/${id}#p${id}`);
    await expect(page.locator('#quickReply')).toHaveCount(0);
    await numbers.getByTitle('Reply to this post', { exact: true }).click();
    await expect(page.locator('#qrCom')).toHaveValue(`>>${id}\n`);
    await expect(page).toHaveURL(`${origin}/fixture/thread/${id}#p${id}`);
    await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
    const response = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0,
      form: { resto: id, com: 'Owned live post-number reply', password } }));
    expect(response.status()).toBe(303); const reply = /#p(\d+)$/.exec(response.headers().location)[1];
    await page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
    await expect(page.locator(`#m${reply}`)).toHaveText('Owned live post-number reply');
    await page.locator(`#pi${reply} > .postNum > a[title="Reply to this post"]`).click();
    await expect(page.locator('#qrCom')).toHaveValue(`>>${reply}\n`);
    await expect(page.locator('#qrResto')).toHaveValue(id);
  } finally {
    await withDeletionQuota(async () => {
      expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0,
        form: { no: id, password } })).status()).toBe(303);
    });
  }
});

test('the reply link prefills and submits a real quote without JavaScript on desktop and mobile', async ({ browser, request }) => {
  const created = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0,
    form: { com: 'Owned script-free quote thread', password } }));
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  const context = await browser.newContext({ javaScriptEnabled: false });
  try {
    const page = await context.newPage();
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(`${origin}/fixture/thread/${id}`);
      await page.locator(`#${width === 390 ? 'pim' : 'pi'}${id} > .postNum > a[title="Reply to this post"]`).click();
      await expect(page).toHaveURL(`${origin}/fixture/thread/${id}?quote=${id}#reply`);
      await expect(page.locator('#com')).toHaveValue(`>>${id}\n`);
      await expect(page.locator('#com')).toBeVisible();
      await page.locator('#com').fill(`>>${id}\nOwned script-free quote at ${width}`);
      await expect(page.locator('#postPassword')).toHaveValue('');
      await withPostingHistory(() => page.locator('form.postEditor button[type="submit"]').click());
      await expect(page).toHaveURL(new RegExp(`/fixture/thread/${id}#p[1-9][0-9]*$`));
      await expect(page.locator('.postMessage').filter({ hasText: `Owned script-free quote at ${width}` })).toHaveCount(1);
    }
    const data = await (await request.get(`/fixture/thread/${id}.json`)).json();
    expect(data.posts).toHaveLength(3);
  } finally {
    await context.close();
    await withDeletionQuota(async () => {
      expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0,
        form: { no: id, password } })).status()).toBe(303);
    });
  }
});

test('disabling Quick Reply keeps the mobile reply form visible and rejects invalid quote targets', async ({ page, request }) => {
  const create = text => withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0,
    form: { com: text, password } }));
  const created = await create('Owned disabled Quick Reply thread'), other = await create('Owned other quote thread');
  expect(created.status()).toBe(303); expect(other.status()).toBe(303);
  const id = /#p(\d+)$/.exec(created.headers().location)[1], foreign = /#p(\d+)$/.exec(other.headers().location)[1];
  try {
    const before = await (await request.get(`/fixture/thread/${id}.json`)).text();
    for (const quote of ['', '0', '01', '-1', '%2B1', '9223372036854775808', '%3Cscript%3E']) {
      expect((await request.get(`/fixture/thread/${id}?quote=${quote}`)).status()).toBe(400);
    }
    expect((await request.get(`/fixture/thread/${id}?quote=${id}&quote=${id}`)).status()).toBe(400);
    expect((await request.get(`/fixture/thread/${id}?quote=${foreign}`)).status()).toBe(404);
    expect(await (await request.get(`/fixture/thread/${id}.json`)).text()).toBe(before);
    await page.setViewportSize({ width: 390, height: 900 });
    await page.goto(`/fixture/thread/${id}`);
    await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: false })));
    await page.reload();
    await page.locator(`#pim${id} > .postNum > a[title="Reply to this post"]`).click();
    await expect(page.locator('#quickReply')).toHaveCount(0);
    await expect(page.locator('#com')).toBeVisible();
    await expect(page.locator('#com')).toHaveValue(`>>${id}\n`);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  } finally {
    for (const no of [id, foreign]) await withDeletionQuota(async () => {
      expect((await request.post('/fixture/delete', { headers: { Origin: origin },
        maxRedirects: 0, form: { no, password } })).status()).toBe(303);
    });
  }
});

async function observePostingBody(page) {
  await page.evaluate(origin => {
    const original = window.fetch;
    let resolve;
    window.ownedPostingResponse = new Promise(done => { resolve = done; });
    window.fetch = async (...args) => {
      const response = await original(...args);
      if (String(args[0]) === `${origin}/fixture/imgboard.php` && args[1]?.method === 'POST') {
        window.fetch = original;
        // Read the same real response in its browser context. A clone leaves
        // the original body intact for the actual Quick Reply parser.
        const text = await response.clone().text();
        resolve({ status: response.status, text });
      }
      return response;
    };
  }, origin);
}

test('a mobile quote form keeps edits made before its client script initializes', async ({ browser, request }) => {
  const created = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0,
    form: { com: 'Owned early mobile form thread', password } }));
  expect(created.status()).toBe(303);
  const id = /#p(\d+)$/.exec(created.headers().location)[1];
  const context = await browser.newContext({ viewport: { width: 390, height: 900 }, isMobile: true, hasTouch: true });
  let release;
  const pending = new Promise(resolve => { release = resolve; });
  let observed;
  const blocked = new Promise(resolve => { observed = resolve; });
  try {
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: false, threadWatcher: false })));
    const page = await context.newPage();
    await page.route('**/native-quick-reply.v1.js', async route => {
      observed();
      await pending;
      await route.continue();
    });
    await page.goto(`${origin}/fixture/thread/${id}?quote=${id}#reply`, { waitUntil: 'commit' });
    await blocked;
    await expect(page.locator('#com')).toHaveValue(`>>${id}\n`);
    await page.locator('#com').fill('Owned edit made before mobile form initialization');
    release();
    await expect(page.locator('form#reply')).toHaveClass(/nativePostForm/);
    await expect(page.locator('#com')).toBeVisible();
    await expect(page.locator('#com')).toHaveValue('Owned edit made before mobile form initialization');
    await withPostingHistory(() => page.locator('form#reply button[type="submit"]').click());
    await expect(page).toHaveURL(new RegExp(`/fixture/thread/${id}#p[1-9][0-9]*$`));
    const saved = await (await request.get(`/fixture/thread/${id}.json`)).json();
    expect(saved.posts).toHaveLength(2);
    expect(saved.posts[1].com).toBe('Owned edit made before mobile form initialization');
  } finally {
    release();
    await context.close();
    await withDeletionQuota(async () => {
      expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0,
        form: { no: id, password } })).status()).toBe(303);
    });
  }
});

test('Quick Reply persists replies, retains failed drafts, tracks own posts and updates without navigation', async ({ page, context, request }) => {
  const created = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0,
    form: { com: 'Owned Quick Reply thread', sub: 'Owned Quick Reply', password } }));
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  try {
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true, keyBinds: true, threadWatcher: true })));
    await page.goto(`/fixture/thread/${id}`); const url = page.url();
    await page.locator('#togglePostFormLink a').click(); await page.locator('#com').fill('Unsubmitted native draft');
    await page.getByRole('heading', { level: 1 }).click(); await page.keyboard.press('q');
    await expect(page.locator('#quickReply')).toBeVisible();
    await expect(page.locator('#qr-pwd')).toHaveValue(''); await page.locator('#qrCom').fill('');
    await withPostingHistory(() => page.locator('#quickReply input[type=submit]').click());
    await expect(page.locator('#qrError')).toHaveText('Error: No text entered.');
    await expect(page.locator('#qr-pwd')).toHaveValue('');
    await page.locator('#qrCom').fill(`>>${id}\nOwned Quick Reply result`);
    await observePostingBody(page);
    const posted = page.evaluate(() => window.ownedPostingResponse);
    const [, response] = await Promise.all([withPostingHistory(() => page.locator('#quickReply input[type=submit]').click()), posted]);
    expect(response.status).toBe(200); expect(response.text.length).toBeLessThanOrEqual(8192);
    const result = JSON.parse(response.text); expect(result.error).toBeUndefined();
    const reply = String(result.pid); expect(String(result.tid)).toBe(id);
    await expect(page.locator('#qrCom')).toHaveValue(''); await expect(page.locator('#quickReply')).toBeVisible();
    await expect(page.locator(`#m${reply}`)).toContainText('Owned Quick Reply result');
    await expect(page.locator(`#m${reply} .quotelink`)).toHaveText(`>>${id} (OP)`);
    await expect(page.locator(`#m${reply} .quotelink`)).toHaveAttribute('href', `/fixture/post/${id}`);
    await expect(page.locator(`#bl_${id} a.quotelink`)).toHaveText(`>>${reply}`);
    await expect(page.locator(`#bl_${id} a.quotelink`)).toHaveAttribute('href', `/fixture/thread/${id}#p${reply}`);
    await expect.poll(() => page.evaluate(({ id, reply }) => JSON.parse(localStorage.getItem(`4chan-track-fixture-${id}`) || '{}')[`>>${reply}`], { id, reply })).toBe(1);
    expect(page.url()).toBe(url); await expect(page.locator('#com')).toHaveValue('Unsubmitted native draft');
    const data = await (await request.get(`/fixture/thread/${id}.json`)).json(); expect(data.posts).toHaveLength(2);
    await page.locator('#qrCom').fill('Preserved on failure');
    await page.route(`**/fixture/imgboard.php`, route => route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"Owned unavailable service"}' }));
    // Bypass the client advisory to exercise failure handling; server checks remain unchanged.
    await withPostingHistory(() => page.locator('#quickReply input[type=submit]').click({ modifiers: ['Shift'] })); await expect(page.locator('#qrError')).toHaveText('Owned unavailable service');
    await expect(page.locator('#qrCom')).toHaveValue('Preserved on failure');
    expect((await (await request.get(`/fixture/thread/${id}.json`)).json()).posts).toHaveLength(2);
    await page.unroute('**/fixture/imgboard.php');
    await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
    await context.clearCookies(); await page.reload();
    await page.getByRole('heading', { level: 1 }).click(); await page.keyboard.press('q'); await page.locator('#qrCom').fill('Second owned reply'); await expect(page.locator('#qr-pwd')).toHaveValue('');
    // Bypass the client advisory for the cookie-reset scenario; server checks remain unchanged.
    await withPostingHistory(() => page.locator('#quickReply input[type=submit]').click({ modifiers: ['Shift'] }));
    await expect(page.locator('.postMessage').filter({ hasText: 'Second owned reply' })).toBeVisible();
  } finally {
    await withDeletionQuota(async () => {
      const removed = await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } }); expect(removed.status()).toBe(303);
    });
  }
});

test('posting CSP permits only the current board handler and preserves healthy denied controls', async ({ page, context, request }) => {
  await page.goto('/demo/');
  const healthy = await request.get('/healthz'); expect(healthy.status()).toBe(200);
  const other = await withPostingHistory(() => request.post('/fixture/imgboard.php', { headers: { Origin: origin, Accept: 'application/json' }, form: { pwd: password, com: '' } }));
  expect(other.status()).toBe(200); expect((await other.json()).error).toBe('Error: New threads require a subject or comment.');
  expect(await page.evaluate(() => fetch('/healthz').then(() => 'allowed', () => 'blocked'))).toBe('blocked');
  expect(await page.evaluate(() => fetch('/fixture/imgboard.php', { method: 'POST' }).then(() => 'allowed', () => 'blocked'))).toBe('blocked');
  const result = await page.evaluate(() => fetch('/demo/imgboard.php', { method: 'POST', headers: { Accept: 'application/json' }, body: new URLSearchParams({ pwd: 'owned-password', com: '' }) }).then(async response => ({ status: response.status, value: await response.json() })));
  expect(result.status).toBe(200); expect(result.value.error).toBe('Error: New threads require a subject or comment.');
  await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: false, keyBinds: true })));
  await page.reload(); await page.getByRole('heading', { level: 1 }).click(); await page.keyboard.press('q'); await expect(page.locator('#quickReply')).toHaveCount(0);
});

for (const additional of [false, true]) {
  test(`automatic updates suppress only the sole Quick Reply post (other reply: ${additional})`, async ({ page, request }) => {
    const write = form => withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password } }));
    const created = await write({ com: 'Owned Quick Reply notification thread' }); expect(created.status()).toBe(303);
    const id = /#p(\d+)$/.exec(created.headers().location)[1];
    try {
      await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true })));
      await page.setViewportSize({ width: 1280, height: 400 }); await page.goto(`/fixture/thread/${id}`);
      const threadUrl = page.url();
      const title = await page.title();
      const time = new Date('2026-09-14T00:00:00Z'); await page.clock.pauseAt(time);
      await page.locator('.threadNav.desktop input[data-cmd="auto"]').first().check(); await page.clock.runFor(9800);
      await page.locator('.open-qr-link').click();
      await page.locator('#qrCom').fill('Owned automatic Quick Reply'); await expect(page.locator('#qr-pwd')).toHaveValue('');
      await observePostingBody(page);
      const posted = page.evaluate(() => window.ownedPostingResponse);
      const [, response] = await Promise.all([withPostingHistory(() => page.locator('#quickReply input[type=submit]').click()), posted]);
      expect(response.status).toBe(200); expect(response.text.length).toBeLessThanOrEqual(8192);
      const value = JSON.parse(response.text); expect(value.error).toBeUndefined();
      expect(String(value.tid)).toBe(id); const reply = String(value.pid);
      await expect(page).toHaveURL(threadUrl);
      await expect(page.locator('#qrCom')).toHaveValue('');
      await expect.poll(() => page.evaluate(({ id, reply }) => JSON.parse(localStorage.getItem(`4chan-track-fixture-${id}`) || '{}')[`>>${reply}`], { id, reply })).toBe(1);
      if (additional) { const other = await write({ resto: id, com: 'Other participant reply' }); expect(other.status()).toBe(303); }
      await page.clock.runFor(200); await expect(page.locator(`#p${reply}`)).toBeAttached();
      await expect(page).toHaveTitle(additional ? `(2) ${title}` : title);
      await expect(page.locator('link[rel="shortcut icon"]')).toHaveAttribute('href', additional
        ? '/static/notifications/favicon-ws-newposts.ico' : '/static/notifications/favicon-ws.ico');
    } finally { await withDeletionQuota(async () => {
      expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303);
    }); }
  });
}

test('a Quick Reply committed during an in-flight update schedules one follow-up snapshot', async ({ page, request }) => {
  const created = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0, form: { com: 'Owned busy update thread', password } }));
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  const path = `/_watch/fixture/thread/${id}/posts`;
  try {
    await page.goto(`/fixture/thread/${id}`); const snapshot = await (await request.get(path)).body();
    const time = new Date('2026-09-14T00:00:00Z'); await page.clock.pauseAt(time);
    let held, calls = 0;
    await page.route(`**${path}`, route => { calls++; if (calls === 1) held = route; else return route.continue(); });
    await page.locator('.threadNav.desktop a[data-cmd="update"]').first().click(); await expect.poll(() => calls).toBe(1);
    await page.locator('.open-qr-link').click();
    await page.locator('#qrCom').fill('Committed while updater was busy'); await expect(page.locator('#qr-pwd')).toHaveValue('');
    await withPostingHistory(() => page.locator('#quickReply input[type=submit]').click()); await expect(page.locator('#quickReply')).toHaveCount(0);
    await page.clock.runFor(600); expect(calls).toBe(1);
    await held.fulfill({ contentType: 'application/json', body: snapshot });
    await expect(page.locator('.nativeUpdaterStatus').first()).toHaveText('No new posts');
    await page.clock.runFor(500); await expect(page.locator('.postMessage').filter({ hasText: 'Committed while updater was busy' })).toBeVisible();
    expect(calls).toBe(2); await page.clock.runFor(2000); expect(calls).toBe(2);
  } finally { await page.unrouteAll({ behavior: 'ignoreErrors' }); await withDeletionQuota(async () => {
    expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303);
  }); }
});

test('the source byte advisory does not block a Unicode reply within the server character limit', async ({ page, request }) => {
  const created = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0, form: { com: 'Owned Unicode advisory thread', password } }));
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  try {
    await page.goto(`/fixture/thread/${id}`);
    const limit = Number(await page.locator('form.postEditor').getAttribute('data-comment-limit')); expect(limit).toBeGreaterThanOrEqual(4);
    const value = '𠮷'.repeat(Math.floor(limit / 4) + 1), bytes = new TextEncoder().encode(value).length;
    await page.locator('.open-qr-link').click();
    await expect(page.locator('#qrResto')).toHaveValue(id);
    await expect(page.locator('#qr-pwd')).toHaveValue(''); await page.locator('#qrCom').fill(value); await page.locator('#qrCom').press('ArrowLeft');
    await expect(page.locator('#qrError')).toHaveText(`Error: Comment too long (${bytes}/${limit}).`);
    await expect(page.locator('#quickReply input[type=submit]')).toBeEnabled();
    await observePostingBody(page);
    const posted = page.evaluate(() => window.ownedPostingResponse);
    const [, response] = await Promise.all([withPostingHistory(() => page.locator('#quickReply input[type=submit]').click()), posted]);
    expect(response.status).toBe(200); expect(response.text.length).toBeLessThanOrEqual(8192);
    const result = JSON.parse(response.text); expect(result.error).toBeUndefined(); expect(String(result.tid)).toBe(id);
    await expect(page.locator('#quickReply')).toHaveCount(0);
    await expect(page.locator(`#m${result.pid}`)).toHaveText(value);
    const scalars = Array.from(value).length;
    const wrapped = `${'𠮷'.repeat(35)}<wbr>`.repeat(Math.floor(scalars / 35)) + '𠮷'.repeat(scalars % 35);
    await expect(page.locator(`#m${result.pid} wbr`)).toHaveCount(Math.floor(scalars / 35));
    const data = await (await request.get(`/fixture/thread/${id}.json`)).json(); expect(data.posts.at(-1).com).toBe(wrapped);
  } finally { await withDeletionQuota(async () => {
    expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303);
  }); }
});

test('Q posts selected text and Ctrl-click works without optional keyboard shortcuts on persisted threads', async ({ page, context, request }) => {
  const selected = 'Owned selected post text';
  const created = await withPostingHistory(() => request.post('/fixture/post', { headers: { Origin: origin }, maxRedirects: 0, form: { com: selected, password } }));
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  try {
    await page.goto(`/fixture/thread/${id}`);
    await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true }))); await page.reload();
    await page.locator(`#m${id}`).evaluate(node => { const range = document.createRange(); range.selectNodeContents(node); getSelection().removeAllRanges(); getSelection().addRange(range); });
    await page.keyboard.press('q'); await expect(page.locator('#qrCom')).toHaveValue(`>${selected}\n`);
    await expect(page.locator('#qr-pwd')).toHaveValue('');
    await observePostingBody(page);
    const posted = page.evaluate(() => window.ownedPostingResponse);
    const [, response] = await Promise.all([withPostingHistory(() => page.locator('#quickReply input[type=submit]').click()), posted]);
    expect(response.status).toBe(200); expect(response.text.length).toBeLessThanOrEqual(8192);
    const result = JSON.parse(response.text); expect(result.error).toBeUndefined(); expect(String(result.tid)).toBe(id);
    await expect(page.locator(`#m${result.pid} .quote`)).toHaveText(`>${selected}`);
    await expect(page.locator(`#m${result.pid} .quotelink`)).toHaveCount(0);
    await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: false }))); await page.reload();
    await page.locator(`#pi${id} > .postNum > a[title="Reply to this post"]`).click({ modifiers: ['Control'] });
    await expect(page.locator('#qrCom')).toHaveValue(''); expect(context.pages()).toHaveLength(1);
    await page.locator('#qrCom').fill('Posted after Ctrl-click'); await expect(page.locator('#qr-pwd')).toHaveValue('');
    // Bypass the client advisory for the shortcut scenario; server checks remain unchanged.
    await withPostingHistory(() => page.locator('#quickReply input[type=submit]').click({ modifiers: ['Shift'] }));
    await expect(page.locator('.postMessage').filter({ hasText: 'Posted after Ctrl-click' })).toBeVisible();
    expect((await (await request.get(`/fixture/thread/${id}.json`)).json()).posts).toHaveLength(3);
  } finally { await withDeletionQuota(async () => {
    expect((await request.post('/fixture/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303);
  }); }
});
