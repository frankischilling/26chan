import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';

async function submit(page, path, text) {
  await page.goto(path);
  await expect(page.locator('#postPassword')).toHaveAttribute('type', 'hidden');
  await expect(page.locator('#postPassword')).toHaveValue('');
  await expect(page.locator('input[type=password]')).toHaveCount(0);
  if (!await page.locator('#com').isVisible()) {
    await page.locator('#togglePostFormLink a:visible, #mpostform a:visible').first().click();
  }
  await page.locator('#com').fill(text);
  await page.locator('form.postEditor button[type=submit]').click();
  await expect(page).toHaveURL(/\/fixture\/thread\/[1-9][0-9]*#p[1-9][0-9]*$/);
  return /#p(\d+)$/.exec(page.url())[1];
}

for (const width of [1280, 390]) {
  test(`automatic anonymous ownership persists through tabs and Quick Reply at ${width}`, async ({ page, context }) => {
    await page.setViewportSize({ width, height: 900 });
    const op = await submit(page, '/fixture/', `Owned anonymous OP at ${width}`);
    try {
      const cookies = await context.cookies(origin);
      const anonymous = cookies.filter(cookie => cookie.name === 'board-anon');
      expect(anonymous).toHaveLength(1);
      expect(anonymous[0]).toMatchObject({ path: '/', httpOnly: true, secure: false, sameSite: 'Strict' });
      expect(anonymous[0].expires - Date.now() / 1000).toBeGreaterThan(31535900);
      expect(await page.evaluate(() => document.cookie)).not.toContain('board-anon=');
      const second = await context.newPage();
      await second.setViewportSize({ width, height: 900 });
      await second.goto(`/fixture/thread/${op}`);
      await second.locator(`#${width === 390 ? 'pim' : 'pi'}${op} a[title="Reply to this post"]`).click();
      await expect(second.locator('#quickReply')).toBeVisible();
      await expect(second.locator('#qr-pwd')).toHaveAttribute('type', 'hidden');
      await expect(second.locator('#qr-pwd')).toHaveValue('');
      await second.locator('#qrCom').fill(`Owned automatic Quick Reply at ${width}`);
      await second.locator('#quickReply input[type=submit]').click();
      await expect(second.locator('.replyContainer .postMessage')).toHaveText(`Owned automatic Quick Reply at ${width}`);
      const thread = await (await context.request.get(`/fixture/thread/${op}.json`)).json();
      expect(thread.posts).toHaveLength(2);
      const reply = String(thread.posts[1].no);
      expect((await context.cookies(origin)).find(cookie => cookie.name === 'board-anon').value).toBe(anonymous[0].value);
      await page.goto(`/fixture/thread/${op}`);
      await page.locator(`#p${reply} summary`).click();
      await expect(page.locator(`#delete${reply}`)).toHaveAttribute('type', 'hidden');
      await page.locator(`#p${reply}`).getByRole('button', { name: 'Delete post', exact: true }).click();
      await expect(page).toHaveURL(`${origin}/fixture/`);
      expect((await (await context.request.get(`/fixture/thread/${op}.json`)).json()).posts).toHaveLength(1);
      await second.close();
    } finally {
      expect((await context.request.post('/fixture/delete', { headers: { Origin: origin }, form: { no: op }, maxRedirects: 0 })).status()).toBe(303);
    }
  });

  test(`ordinary forms post and delete without JavaScript or a password at ${width}`, async ({ browser }) => {
    const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width, height: 900 } });
    try {
      const page = await context.newPage();
      const op = await submit(page, `${origin}/fixture/`, `Owned script-free anonymous OP at ${width}`);
      const reply = await submit(page, `${origin}/fixture/thread/${op}`, `Owned script-free anonymous reply at ${width}`);
      expect((await (await context.request.get(`${origin}/fixture/thread/${op}.json`)).json()).posts).toHaveLength(2);
      await page.locator(`#p${reply} summary`).click();
      await page.locator(`#p${reply}`).getByRole('button', { name: 'Delete post', exact: true }).click();
      await expect(page).toHaveURL(`${origin}/fixture/`);
      expect((await context.request.post(`${origin}/fixture/delete`, { headers: { Origin: origin }, form: { no: op }, maxRedirects: 0 })).status()).toBe(303);
    } finally { await context.close(); }
  });
}

test('rejecting browser storage preserves posting but cannot retain deletion authority', async ({ page, context, request }) => {
  await context.addInitScript(() => {
    Object.defineProperty(window, 'localStorage', { get() { throw new DOMException('Owned rejected storage', 'SecurityError'); } });
  });
  const op = await submit(page, '/fixture/', 'Owned post with unavailable browser storage');
  const saved = await context.cookies(origin);
  try {
    const thread = await (await request.get(`/fixture/thread/${op}.json`)).json();
    expect(thread.posts).toHaveLength(1);
    await context.clearCookies();
    await page.goto(`/fixture/thread/${op}`);
    const denied = await context.request.post('/fixture/delete', { headers: { Origin: origin }, form: { no: op }, maxRedirects: 0 });
    expect(denied.status()).toBe(403);
    expect((await request.get(`/fixture/thread/${op}.json`)).status()).toBe(200);
  } finally {
    await context.addCookies(saved);
    expect((await context.request.post('/fixture/delete', { headers: { Origin: origin }, form: { no: op }, maxRedirects: 0 })).status()).toBe(303);
  }
});

test('display tracking and a replacement anonymous session cannot delete another session’s post', async ({ page, context, browser }) => {
  const op = await submit(page, '/fixture/', 'Owned identity before cookie reset');
  const saved = await context.cookies(origin);
  const outsider = await browser.newContext();
  let replacement;
  try {
    await outsider.addCookies([{ name: `board-posted-${op}`, value: `${op}.1`, url: origin }, { name: '4chan_awt', value: op, url: origin }]);
    expect((await outsider.request.post(`${origin}/fixture/delete`, { headers: { Origin: origin }, form: { no: op }, maxRedirects: 0 })).status()).toBe(403);
    await context.clearCookies();
    replacement = await submit(page, '/fixture/', 'Owned identity after cookie reset');
    const current = (await context.cookies(origin)).find(cookie => cookie.name === 'board-anon');
    expect(current.value).not.toBe(saved.find(cookie => cookie.name === 'board-anon').value);
    expect((await context.request.post('/fixture/delete', { headers: { Origin: origin }, form: { no: op }, maxRedirects: 0 })).status()).toBe(403);
    expect((await context.request.post('/fixture/delete', { headers: { Origin: origin }, form: { no: replacement }, maxRedirects: 0 })).status()).toBe(303);
    replacement = undefined;
  } finally {
    if (replacement) await context.request.post('/fixture/delete', { headers: { Origin: origin }, form: { no: replacement }, maxRedirects: 0 });
    await context.addCookies(saved);
    expect((await context.request.post('/fixture/delete', { headers: { Origin: origin }, form: { no: op }, maxRedirects: 0 })).status()).toBe(303);
    await outsider.close();
  }
});
