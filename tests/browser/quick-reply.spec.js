import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000', password = 'owned-quick-reply-password';
test('Quick Reply persists replies, retains failed drafts, tracks own posts and updates without navigation', async ({ page, context, request }) => {
  const created = await request.post('/test/post', { headers: { Origin: origin }, maxRedirects: 0,
    form: { com: 'Owned Quick Reply thread', sub: 'Owned Quick Reply', password } });
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  try {
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true, keyBinds: true, threadWatcher: true })));
    await page.goto(`/test/thread/${id}`); const url = page.url();
    await page.locator('#togglePostFormLink a').click(); await page.locator('#com').fill('Unsubmitted native draft');
    await page.locator('h1').click(); await page.keyboard.press('q');
    await expect(page.locator('#quickReply')).toBeVisible();
    await page.locator('#qr-pwd').fill(password); await page.locator('#qrCom').fill('');
    await page.locator('#quickReply input[type=submit]').click();
    await expect(page.locator('#qrError')).toContainText('comment');
    await expect(page.locator('#qr-pwd')).toHaveValue(password);
    await page.locator('#qrCom').fill(`>>${id}\nOwned Quick Reply result`);
    const posted = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${origin}/test/imgboard.php`);
    await page.locator('#quickReply input[type=submit]').click();
    const response = await posted; expect(response.status()).toBe(200); const result = await response.json();
    const reply = String(result.pid); expect(String(result.tid)).toBe(id);
    await expect(page.locator('#qrCom')).toHaveValue(''); await expect(page.locator('#quickReply')).toBeVisible();
    await expect(page.locator(`#m${reply}`)).toContainText('Owned Quick Reply result');
    await expect(page.locator(`#m${reply} .quotelink`)).toHaveText(`>>${id}`);
    await expect.poll(() => page.evaluate(({ id, reply }) => JSON.parse(localStorage.getItem(`4chan-track-test-${id}`) || '{}')[`>>${reply}`], { id, reply })).toBe(1);
    expect(page.url()).toBe(url); await expect(page.locator('#com')).toHaveValue('Unsubmitted native draft');
    const data = await (await request.get(`/test/thread/${id}.json`)).json(); expect(data.posts).toHaveLength(2);
    await page.locator('#qrCom').fill('Preserved on failure');
    await page.route(`**/test/imgboard.php`, route => route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"Owned unavailable service"}' }));
    await page.locator('#quickReply input[type=submit]').click(); await expect(page.locator('#qrError')).toHaveText('Owned unavailable service');
    await expect(page.locator('#qrCom')).toHaveValue('Preserved on failure');
    expect((await (await request.get(`/test/thread/${id}.json`)).json()).posts).toHaveLength(2);
    await page.unroute('**/test/imgboard.php');
    await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
    await context.clearCookies(); await page.reload();
    await page.locator('h1').click(); await page.keyboard.press('q'); await page.locator('#qrCom').fill('Second owned reply'); await page.locator('#qr-pwd').fill(password);
    await page.locator('#quickReply input[type=submit]').click();
    await expect(page.locator('.postMessage').filter({ hasText: 'Second owned reply' })).toBeVisible();
  } finally {
    const removed = await request.post('/test/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } }); expect(removed.status()).toBe(303);
  }
});

test('posting CSP permits only the current board handler and preserves healthy denied controls', async ({ page, context, request }) => {
  await page.goto('/demo/');
  const healthy = await request.get('/healthz'); expect(healthy.status()).toBe(200);
  const other = await request.post('/test/imgboard.php', { headers: { Origin: origin, Accept: 'application/json' }, form: { pwd: password, com: '' } });
  expect(other.status()).toBe(200); expect((await other.json()).error).toContain('comment');
  expect(await page.evaluate(() => fetch('/healthz').then(() => 'allowed', () => 'blocked'))).toBe('blocked');
  expect(await page.evaluate(() => fetch('/test/imgboard.php', { method: 'POST' }).then(() => 'allowed', () => 'blocked'))).toBe('blocked');
  const result = await page.evaluate(() => fetch('/demo/imgboard.php', { method: 'POST', headers: { Accept: 'application/json' }, body: new URLSearchParams({ pwd: 'owned-password', com: '' }) }).then(async response => ({ status: response.status, value: await response.json() })));
  expect(result.status).toBe(200); expect(result.value.error).toContain('comment');
  await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: false, keyBinds: true })));
  await page.reload(); await page.locator('h1').click(); await page.keyboard.press('q'); await expect(page.locator('#quickReply')).toHaveCount(0);
});

for (const additional of [false, true]) {
  test(`automatic updates suppress only the sole Quick Reply post (other reply: ${additional})`, async ({ page, request }) => {
    const write = form => request.post('/test/post', { headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password } });
    const created = await write({ com: 'Owned Quick Reply notification thread' }); expect(created.status()).toBe(303);
    const id = /#p(\d+)$/.exec(created.headers().location)[1];
    try {
      await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true })));
      await page.setViewportSize({ width: 1280, height: 400 }); await page.goto(`/test/thread/${id}`);
      const title = await page.title();
      const time = new Date('2026-09-14T00:00:00Z'); await page.clock.install({ time }); await page.clock.pauseAt(time);
      await page.locator('.threadNav.desktop input[data-cmd="auto"]').first().check(); await page.clock.runFor(9800);
      await page.locator('.open-qr-link').click();
      await page.locator('#qrCom').fill('Owned automatic Quick Reply'); await page.locator('#qr-pwd').fill(password);
      const posted = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${origin}/test/imgboard.php`);
      await page.locator('#quickReply input[type=submit]').click(); const reply = String((await (await posted).json()).pid);
      await expect(page.locator('#qrCom')).toHaveValue('');
      await expect.poll(() => page.evaluate(({ id, reply }) => JSON.parse(localStorage.getItem(`4chan-track-test-${id}`) || '{}')[`>>${reply}`], { id, reply })).toBe(1);
      if (additional) { const other = await write({ resto: id, com: 'Other participant reply' }); expect(other.status()).toBe(303); }
      await page.clock.runFor(200); await expect(page.locator(`#p${reply}`)).toBeAttached();
      await expect(page).toHaveTitle(additional ? `(2) ${title}` : title);
      await expect(page.locator('link[rel="shortcut icon"]')).toHaveAttribute('href', additional
        ? '/static/notifications/favicon-ws-newposts.ico' : '/static/notifications/favicon-ws.ico');
    } finally { expect((await request.post('/test/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303); }
  });
}

test('a Quick Reply committed during an in-flight update schedules one follow-up snapshot', async ({ page, request }) => {
  const created = await request.post('/test/post', { headers: { Origin: origin }, maxRedirects: 0, form: { com: 'Owned busy update thread', password } });
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  const path = `/_watch/test/thread/${id}/posts`;
  try {
    await page.goto(`/test/thread/${id}`); const snapshot = await (await request.get(path)).body();
    const time = new Date('2026-09-14T00:00:00Z'); await page.clock.install({ time }); await page.clock.pauseAt(time);
    let held, calls = 0;
    await page.route(`**${path}`, route => { calls++; if (calls === 1) held = route; else return route.continue(); });
    await page.locator('.threadNav.desktop a[data-cmd="update"]').first().click(); await expect.poll(() => calls).toBe(1);
    await page.locator('.open-qr-link').click();
    await page.locator('#qrCom').fill('Committed while updater was busy'); await page.locator('#qr-pwd').fill(password);
    await page.locator('#quickReply input[type=submit]').click(); await expect(page.locator('#quickReply')).toHaveCount(0);
    await page.clock.runFor(600); expect(calls).toBe(1);
    await held.fulfill({ contentType: 'application/json', body: snapshot });
    await expect(page.locator('.nativeUpdaterStatus').first()).toHaveText('No new posts');
    await page.clock.runFor(500); await expect(page.locator('.postMessage').filter({ hasText: 'Committed while updater was busy' })).toBeVisible();
    expect(calls).toBe(2); await page.clock.runFor(2000); expect(calls).toBe(2);
  } finally { await page.unrouteAll({ behavior: 'ignoreErrors' }); expect((await request.post('/test/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303); }
});

test('the source byte advisory does not block a Unicode reply within the server character limit', async ({ page, request }) => {
  const created = await request.post('/test/post', { headers: { Origin: origin }, maxRedirects: 0, form: { com: 'Owned Unicode advisory thread', password } });
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  try {
    await page.goto(`/test/thread/${id}`);
    const limit = Number(await page.locator('form.postEditor').getAttribute('data-comment-limit')); expect(limit).toBeGreaterThanOrEqual(4);
    const value = '😀'.repeat(Math.floor(limit / 4) + 1), bytes = new TextEncoder().encode(value).length;
    await page.locator('.open-qr-link').click();
    await expect(page.locator('#qrResto')).toHaveValue(id);
    await page.locator('#qr-pwd').fill(password); await page.locator('#qrCom').fill(value); await page.locator('#qrCom').press('ArrowLeft');
    await expect(page.locator('#qrError')).toHaveText(`Error: Comment too long (${bytes}/${limit}).`);
    await expect(page.locator('#quickReply input[type=submit]')).toBeEnabled();
    const posted = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${origin}/test/imgboard.php`);
    await page.locator('#quickReply input[type=submit]').click(); const response = await posted;
    expect(response.status()).toBe(200); const result = await response.json(); expect(String(result.tid)).toBe(id);
    await expect(page.locator('#quickReply')).toHaveCount(0);
    await expect(page.locator(`#m${result.pid}`)).toHaveText(value);
    const data = await (await request.get(`/test/thread/${id}.json`)).json(); expect(data.posts.at(-1).com).toBe(value);
  } finally { expect((await request.post('/test/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303); }
});

test('Q posts selected text and Ctrl-click works without optional keyboard shortcuts on persisted threads', async ({ page, context, request }) => {
  const selected = 'Owned selected post text';
  const created = await request.post('/test/post', { headers: { Origin: origin }, maxRedirects: 0, form: { com: selected, password } });
  expect(created.status()).toBe(303); const id = /#p(\d+)$/.exec(created.headers().location)[1];
  try {
    await page.goto(`/test/thread/${id}`);
    await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true }))); await page.reload();
    await page.locator(`#m${id}`).evaluate(node => { const range = document.createRange(); range.selectNodeContents(node); getSelection().removeAllRanges(); getSelection().addRange(range); });
    await page.keyboard.press('q'); await expect(page.locator('#qrCom')).toHaveValue(`>${selected}\n`);
    await page.locator('#qr-pwd').fill(password);
    const posted = page.waitForResponse(response => response.request().method() === 'POST' && response.url() === `${origin}/test/imgboard.php`);
    await page.locator('#quickReply input[type=submit]').click(); const result = await (await posted).json(); expect(String(result.tid)).toBe(id);
    await expect(page.locator(`#m${result.pid} .quote`)).toHaveText(`>${selected}`);
    await expect(page.locator(`#m${result.pid} .quotelink`)).toHaveCount(0);
    await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: false }))); await page.reload();
    await page.locator(`#pi${id} > .postNum`).click({ modifiers: ['Control'] });
    await expect(page.locator('#qrCom')).toHaveValue(''); expect(context.pages()).toHaveLength(1);
    await page.locator('#qrCom').fill('Posted after Ctrl-click'); await page.locator('#qr-pwd').fill(password);
    await page.locator('#quickReply input[type=submit]').click();
    await expect(page.locator('.postMessage').filter({ hasText: 'Posted after Ctrl-click' })).toBeVisible();
    expect((await (await request.get(`/test/thread/${id}.json`)).json()).posts).toHaveLength(3);
  } finally { expect((await request.post('/test/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } })).status()).toBe(303); }
});
