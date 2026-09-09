import { test, expect } from '@playwright/test';

const apiOrigin = 'http://127.0.0.1:3003';

async function apiClient(page, origin = 'http://127.0.0.1:3000') {
  // A controlled client document keeps production pages' script/connect CSP
  // intact. Only this document is intercepted; all API traffic uses real HTTP.
  const url = `${origin}/__synthetic_api_client`;
  // Chromium assigns an unknown address space to an intercepted document.
  // Grant the local-device permission to this test origin; CORS stays enabled.
  await page.context().grantPermissions(['local-network-access'], { origin });
  await page.route(url, route => route.fulfill({
    contentType: 'text/html',
    headers: { 'content-security-policy': `default-src 'none'; connect-src ${apiOrigin}` },
    body: '<!doctype html><title>Synthetic API client</title>',
  }));
  await page.goto(url);
}

test('board-origin browser client reads real JSON and revalidates exposed cache headers', async ({ page }) => {
  await apiClient(page);
  const result = await page.evaluate(async api => {
    const path = `${api}/demo/thread/1000001.json`;
    const first = await fetch(path, { credentials: 'omit', cache: 'no-store' });
    const body = await first.json();
    const etag = first.headers.get('etag');
    const modified = first.headers.get('last-modified');
    const conditional = await fetch(path, {
      credentials: 'omit', cache: 'no-store', headers: { 'If-None-Match': etag },
    });
    const modifiedConditional = await fetch(path, {
      credentials: 'omit', cache: 'no-store',
      headers: { 'If-Modified-Since': new Date(Date.parse(modified) + 1000).toUTCString() },
    });
    const head = await fetch(path, { method: 'HEAD', credentials: 'omit', cache: 'no-store' });
    const missing = await fetch(`${api}/demo/thread/9223372036854775807.json`, { credentials: 'omit' });
    return {
      status: first.status, no: body.posts[0].no, etag, modified,
      conditional: conditional.status, conditionalBody: await conditional.text(),
      conditionalEtag: conditional.headers.get('etag'),
      modifiedConditional: modifiedConditional.status,
      modifiedConditionalBody: await modifiedConditional.text(),
      head: head.status, headBody: await head.text(), missing: missing.status,
    };
  }, apiOrigin);
  expect(result.status).toBe(200);
  expect(result.no).toBe(1000001);
  expect(result.etag).toMatch(/^"[a-f0-9]{64}"$/);
  expect(result.modified).toBe('Tue, 08 Sep 2026 12:05:00 GMT');
  expect(result.conditional).toBe(304);
  expect(result.conditionalBody).toBe('');
  expect(result.conditionalEtag).toBe(result.etag);
  expect(result.modifiedConditional).toBe(304);
  expect(result.modifiedConditionalBody).toBe('');
  expect(result.head).toBe(200);
  expect(result.headBody).toBe('');
  expect(result.missing).toBe(404);
});

test('browser CORS denies unapproved and credentialed clients with a healthy allowed control', async ({ page }) => {
  // Positive control proves the same destination is available, including for
  // requests rejected by the browser's CORS implementation below.
  await apiClient(page);
  expect(await page.evaluate(async api => (await fetch(`${api}/boards.json`, { credentials: 'omit' })).status, apiOrigin)).toBe(200);
  const credentialed = await page.evaluate(async api => {
    try { await fetch(`${api}/boards.json`, { credentials: 'include' }); return 'readable'; }
    catch (error) { return error.name; }
  }, apiOrigin);
  expect(credentialed).toBe('TypeError');
  await apiClient(page, 'http://localhost:3000');
  const denied = await page.evaluate(async api => {
    try { await fetch(`${api}/boards.json`, { credentials: 'omit' }); return 'readable'; }
    catch (error) { return error.name; }
  }, apiOrigin);
  expect(denied).toBe('TypeError');
});

test('API listener rejects posting and browser preflights cannot grant write access', async ({ page }) => {
  await apiClient(page);
  const result = await page.evaluate(async api => {
    const before = await (await fetch(`${api}/demo/thread/1000001.json`, { credentials: 'omit', cache: 'no-store' })).json();
    let post;
    try {
      const response = await fetch(`${api}/demo/post`, {
        method: 'POST', credentials: 'omit',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: 'resto=1000001&com=A+synthetic+API+write+must+not+persist&password=test-password',
      });
      post = response.status;
    } catch { post = 'blocked'; }
    let preflight;
    try {
      await fetch(`${api}/boards.json`, { method: 'DELETE', credentials: 'omit' });
      preflight = 'readable';
    } catch (error) { preflight = error.name; }
    const after = await (await fetch(`${api}/demo/thread/1000001.json`, { credentials: 'omit', cache: 'no-store' })).json();
    return { post, preflight, before: before.posts, after: after.posts };
  }, apiOrigin);
  expect(['blocked', 404, 405]).toContain(result.post);
  expect(result.preflight).toBe('TypeError');
  expect(result.after).toEqual(result.before);
});

test('posting, replying, reporting, and password deletion persist through reload', async ({ page }) => {
  await page.goto('/test/');
  await page.locator('#sub').fill('A synthetic browser thread');
  await page.locator('#com').fill('>hello\n<script>window.hostile = true</script>\n[spoiler]a hidden fold[/spoiler]');
  await page.locator('#password').fill('browser-password-123');
  await page.getByRole('button', { name: 'Post', exact: true }).click();
  await expect(page).toHaveURL(/\/test\/thread\/\d+#p\d+$/);
  const originalUrl = page.url();
  const op = /#p(\d+)$/.exec(originalUrl)[1];
  await page.reload();
  await expect(page.locator(`#m${op}`)).toContainText('<script>window.hostile = true</script>');
  expect(await page.evaluate(() => window.hostile)).toBeUndefined();
  await expect(page.locator(`#m${op} script`)).toHaveCount(0);
  await page.locator('#com').fill(`>>${op}\nA persisted reply.`);
  await page.locator('#password').fill('browser-reply-password');
  await page.getByRole('button', { name: 'Post', exact: true }).click();
  await expect(page.locator('.replyContainer')).toHaveCount(1);
  await page.locator('.replyContainer .quotelink').click();
  await expect(page).toHaveURL(new RegExp(`#p${op}$`));
  await page.locator(`#p${op} summary`).click();
  await page.locator(`#report${op}`).fill('Synthetic reporting check');
  await page.locator(`#p${op}`).getByRole('button', { name: 'Report post', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Report received' })).toBeVisible();
  await page.goto(originalUrl);
  await page.locator(`#p${op} summary`).click();
  await page.locator(`#delete${op}`).fill('browser-password-123');
  await page.locator(`#p${op}`).getByRole('button', { name: 'Delete post', exact: true }).click();
  await expect(page).toHaveURL(/\/test\/$/);
  const deleted = await page.request.get(`/test/thread/${op}.json`);
  expect(deleted.status()).toBe(404);
});

test('advertised Unicode posting limit works with JavaScript disabled', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto('http://127.0.0.1:3000/demo/');
  await expect(page.getByRole('heading', { name: '/demo/ - Paper craft' })).toBeVisible();
  await page.getByRole('link', { name: 'Reply', exact: true }).click();
  await expect(page.locator('#postForm')).toBeVisible();
  await page.goto('http://127.0.0.1:3000/test/');
  const listing = await (await page.request.get('http://127.0.0.1:3000/boards.json')).json();
  const limit = listing.boards.find(board => board.board === 'test').max_comment_chars;
  expect(limit).toBe(4000);
  const comment = '😀'.repeat(limit);
  await expect(page.locator('#postHelp')).toContainText(`${limit} characters`);
  await expect(page.locator('#com')).not.toHaveAttribute('maxlength');
  await page.locator('#com').fill(comment);
  await page.locator('#password').fill('no-javascript-password');
  await page.getByRole('button', { name: 'Post', exact: true }).click();
  await expect(page).toHaveURL(/\/test\/thread\/\d+#p\d+$/);
  const op = /#p(\d+)$/.exec(page.url())[1];
  const threadUrl = page.url();
  const jsonUrl = `http://127.0.0.1:3000/test/thread/${op}.json`;
  await page.reload();
  await expect(page.locator(`#m${op}`)).toHaveText(comment);
  const before = await page.request.get(jsonUrl);
  const beforeJson = await before.json();
  expect(beforeJson.posts[0].com).toBe(comment);
  expect(beforeJson.posts[0].replies).toBe(0);
  await page.locator('#com').fill(`${comment}a`);
  await page.locator('#password').fill('no-javascript-password');
  const denied = page.waitForResponse(response => response.url().endsWith('/test/post') && response.request().method() === 'POST');
  await page.getByRole('button', { name: 'Post', exact: true }).click();
  expect((await denied).status()).toBe(422);
  const after = await page.request.get(jsonUrl);
  expect(await after.json()).toEqual(beforeJson);
  expect(after.headers().etag).toBe(before.headers().etag);
  await page.goto(threadUrl);
  await page.locator(`#p${op} summary`).click();
  await page.locator(`#delete${op}`).fill('no-javascript-password');
  await page.locator(`#p${op}`).getByRole('button', { name: 'Delete post', exact: true }).click();
  await expect(page).toHaveURL(/\/test\/$/);
  expect((await page.request.get(`/test/thread/${op}.json`)).status()).toBe(404);
  await context.close();
});
