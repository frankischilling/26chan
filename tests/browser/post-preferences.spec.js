import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000', password = 'owned-post-preferences-password';

async function create(context, name = '') {
  const response = await withPostingHistory(() => context.request.post('/demo/post', { headers: { Origin: origin }, maxRedirects: 0,
    form: { name, email: 'sage', com: 'Owned preference thread', sub: 'Owned preferences', password } }));
  expect(response.status()).toBe(303);
  return response.headers().location.match(/#p(\d+)$/)[1];
}

async function remove(context, thread) {
  await withDeletionQuota(async () => {
    expect((await context.request.post('/demo/delete', { headers: { Origin: origin }, maxRedirects: 0,
      form: { no: thread, password } })).status()).toBe(303);
  });
}

test('successful posting restores display preferences into both editors and keeps passwords empty', async ({ page, context }) => {
  const thread = await create(context, '<owned name>#password');
  try {
    const cookies = await context.cookies(origin);
    expect(cookies.filter(cookie => ['4chan_name', 'options'].includes(cookie.name)).map(cookie =>
      [cookie.name, cookie.value, cookie.path, cookie.sameSite, cookie.httpOnly])).toEqual([
      ['4chan_name', '%3Cowned%20name%3E', '/', 'Strict', false], ['options', 'sage', '/', 'Strict', false],
    ]);
    expect(cookies.some(cookie => cookie.value.includes('password'))).toBe(false);
    await page.goto(`/demo/thread/${thread}`);
    await expect(page.locator('#name')).toHaveValue('<owned name>');
    await expect(page.locator('#email')).toHaveValue('sage'); await expect(page.locator('#postPassword')).toHaveValue('');
    for (const cookie of cookies.filter(cookie => ['4chan_name', 'options'].includes(cookie.name))) {
      expect(cookie.expires - Date.now() / 1000).toBeGreaterThan(604700);
      expect(cookie.expires - Date.now() / 1000).toBeLessThanOrEqual(604800);
    }
    await expect(page.locator('owned')).toHaveCount(0);
    await page.locator('.open-qr-link').click();
    await expect(page.locator('#qr-name')).toHaveValue('<owned name>'); await expect(page.locator('#qrEmail')).toHaveValue('sage');
    await expect(page.locator('#qr-pwd')).toHaveValue('');
    await page.locator('#qr-name').fill('Unsubmitted Quick Reply identity');
    await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
    await page.locator('#togglePostFormLink a').click(); await page.locator('#name').fill('Unsubmitted ordinary identity');
    await page.locator('#email').fill('nonoko'); await page.locator('.open-qr-link').click();
    await expect(page.locator('#qr-name')).toHaveValue('Unsubmitted ordinary identity');
    await expect(page.locator('#qrEmail')).toHaveValue('nonoko');
    const failed = await withPostingHistory(() => context.request.post('/demo/imgboard.php', { headers: { Origin: origin, Accept: 'application/json' },
      form: { resto: thread, name: 'Changed##owned-private-secret', com: 'Owned failed identity', email: 'nonoko', pwd: password } }));
    expect((await failed.json()).error).toBe('Secure tripcodes are unavailable.');
    expect(failed.headers()['set-cookie']).toBeUndefined();
    expect((await context.cookies(origin)).filter(cookie => ['4chan_name', 'options'].includes(cookie.name))).toEqual(
      cookies.filter(cookie => ['4chan_name', 'options'].includes(cookie.name)));
  } finally { await remove(context, thread); }
});

test('manually supplied cookies are bounded display text and ambiguous names use an empty default', async ({ page, context }) => {
  const thread = await create(context);
  try {
    await context.addCookies([{ name: '4chan_name', value: encodeURIComponent('<img src=x onerror=owned>#private'), url: origin },
      { name: 'options', value: '%C0%AF', url: origin }]);
    await page.goto(`/demo/thread/${thread}`);
    await expect(page.locator('#name')).toHaveValue('<img src=x onerror=owned>'); await expect(page.locator('#email')).toHaveValue('');
    await expect(page.locator('img[src="x"]')).toHaveCount(0);
    await context.addCookies([{ name: '4chan_name', value: 'Conflicting', domain: '127.0.0.1', path: '/demo/' }]);
    await page.reload(); await expect(page.locator('#name')).toHaveValue('');
  } finally { await remove(context, thread); }
});

test('an unavailable cookie reader leaves posting controls usable', async ({ page, context }) => {
  const thread = await create(context, 'Owned remembered name');
  try {
    await page.addInitScript(() => Object.defineProperty(document, 'cookie', {
      get() { throw new DOMException('Owned cookie denial', 'SecurityError'); }, set() {}, configurable: true,
    }));
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.goto(`/demo/thread/${thread}`);
    await page.locator('.open-qr-link').click(); await expect(page.locator('#quickReply')).toBeVisible();
    await expect(page.locator('#qr-name')).toHaveValue('');
    await expect(page.locator('#qr-pwd')).toHaveValue(''); await page.locator('#qrCom').fill('Owned cookie-reader-denied reply');
    await withPostingHistory(() => page.locator('#quickReply input[type=submit]').click()); await expect(page.locator('#quickReply')).toHaveCount(0);
    const posts = (await (await context.request.get(`/demo/thread/${thread}.json`)).json()).posts;
    expect(posts).toHaveLength(2); expect(posts[1].name).toBe('Anonymous');
    expect(errors).toEqual([]);
  } finally { await remove(context, thread); }
});
