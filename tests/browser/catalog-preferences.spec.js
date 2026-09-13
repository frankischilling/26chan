import { test, expect } from '@playwright/test';

const catalog = '/test/catalog';
const key = 'catalog-settings';
const saved = { orderby: 'r', large: true, extended: false };
const stored = page => page.evaluate(key => JSON.parse(localStorage.getItem(key)), key);

test('catalog preferences save on changes and restore in a fresh tab without storing search text', async ({ page, context }) => {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(catalog);
  await page.locator('#order-ctrl').selectOption('r');
  await expect(page).toHaveURL(/order=r/);
  await page.locator('#size-ctrl').selectOption('large');
  await expect(page.locator('#threads')).toHaveClass('catalog extended-large');
  await page.locator('#teaser-ctrl').selectOption('off');
  await expect(page.locator('#threads')).toHaveClass('catalog large');
  await page.locator('#qf-box').fill('private search [.*] <script>');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  expect(await stored(page)).toEqual(saved);
  const fresh = await context.newPage();
  await fresh.goto(catalog);
  await expect(fresh.locator('#threads')).toHaveClass('catalog large');
  await expect(fresh.locator('#order-ctrl')).toHaveValue('r');
  await expect(fresh.locator('#qf-box')).toHaveValue('');
  await fresh.reload();
  await expect(fresh.locator('#threads')).toHaveClass('catalog large');
  await fresh.getByRole('link', { name: 'Reset', exact: true }).click();
  await expect(fresh.locator('#threads')).toHaveClass('catalog extended-small');
  expect(await stored(fresh)).toBeNull();
  await fresh.goto(catalog);
  await expect(fresh).toHaveURL(new URL(catalog, fresh.url()).href);
  await expect(fresh.locator('#order-ctrl')).toHaveValue('alt');
  expect(errors).toEqual([]);
});

test('explicit catalog URLs override but do not overwrite saved preferences', async ({ page }) => {
  await page.goto(catalog);
  await page.evaluate(({ key, saved }) => localStorage.setItem(key, JSON.stringify(saved)), { key, saved });
  await page.goto(`${catalog}?order=date&size=small&teaser=on&q=literal`);
  await expect(page.locator('#threads')).toHaveClass('catalog extended-small');
  await expect(page.locator('#order-ctrl')).toHaveValue('date');
  await expect(page.locator('#qf-box')).toHaveValue('literal');
  expect(await stored(page)).toEqual(saved);
  await page.goto(`${catalog}?q=literal%20%5B.*%5D#threads`);
  await expect(page.locator('#threads')).toHaveClass('catalog large');
  await expect(page.locator('#qf-box')).toHaveValue('literal [.*]');
  expect(new URL(page.url()).hash).toBe('#threads');
  expect(await stored(page)).toEqual(saved);
});

test('malformed and oversized stored preferences cannot become navigation or executable content', async ({ page }) => {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(catalog);
  for (const raw of ['', '{', 'null', '[]', 'true', JSON.stringify({ orderby: 'r' }),
    JSON.stringify({ ...saved, large: 'true' }), JSON.stringify({ ...saved, extended: 0 }),
    JSON.stringify({ ...saved, orderby: 'https://example.invalid/<script>' }),
    JSON.stringify({ ...saved, padding: 'x'.repeat(1024) })]) {
    await page.evaluate(({ key, raw }) => localStorage.setItem(key, raw), { key, raw });
    await page.goto(catalog);
    await expect(page.locator('#threads')).toHaveClass('catalog extended-small');
    await expect(page.locator('#order-ctrl')).toHaveValue('alt');
    expect(new URL(page.url()).search).toBe('');
  }
  await page.evaluate(key => localStorage.setItem(key, '{"orderby":"r","large":true,"extended":false,"__proto__":{"polluted":true},"q":"private","url":"https://example.invalid"}'), key);
  await page.goto(catalog);
  await expect(page.locator('#threads')).toHaveClass('catalog large');
  expect(new URL(page.url()).origin).toBe('http://127.0.0.1:3000');
  expect(await page.evaluate(() => ({}).polluted)).toBeUndefined();
  await expect(page.locator('#qf-box')).toHaveValue('');
  expect(errors).toEqual([]);
});

test('unavailable browser storage leaves controls and reset usable', async ({ page, context }) => {
  await context.addInitScript(() => {
    for (const name of ['getItem', 'setItem', 'removeItem']) {
      Object.defineProperty(Storage.prototype, name, { value() { throw new DOMException('Storage unavailable', 'SecurityError'); } });
    }
  });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(catalog);
  await page.locator('#size-ctrl').selectOption('large');
  await expect(page.locator('#threads')).toHaveClass('catalog extended-large');
  await page.getByRole('link', { name: 'Reset', exact: true }).click();
  await expect(page.locator('#threads')).toHaveClass('catalog extended-small');
  expect(errors).toEqual([]);
});

test('catalog CSP permits only the fixed script and denies healthy alternate and inline scripts', async ({ page, context }) => {
  let alternateRequests = 0;
  await context.route('**/static/catalog-denied.js', route => {
    alternateRequests += 1;
    return route.fulfill({ contentType: 'text/javascript', body: 'window.alternateExecuted = true;' });
  });
  await context.route('**/catalog-script-control', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><script src="/static/catalog-denied.js"></script>' }));
  await page.goto('/catalog-script-control');
  expect(await page.evaluate(() => window.alternateExecuted)).toBe(true);
  expect(alternateRequests).toBe(1);
  const script = page.waitForResponse(response => response.url().endsWith('/static/catalog-preferences.v1.js'));
  const response = await page.goto(catalog);
  expect((await script).status()).toBe(200);
  expect(response.headers()['content-security-policy']).toContain("script-src http://127.0.0.1:3000/static/catalog-preferences.v1.js;");
  await page.evaluate(() => {
    window.violations = [];
    document.addEventListener('securitypolicyviolation', event => window.violations.push(event.blockedURI));
    const alternate = document.createElement('script');
    alternate.src = '/static/catalog-denied.js';
    const inline = document.createElement('script');
    inline.textContent = 'window.inlineExecuted = true;';
    document.body.append(alternate, inline);
  });
  await expect.poll(() => page.evaluate(() => window.violations.length)).toBe(2);
  expect(await page.evaluate(() => [window.alternateExecuted, window.inlineExecuted])).toEqual([undefined, undefined]);
  expect(alternateRequests).toBe(1);
  await page.locator('#size-ctrl').selectOption('large');
  await expect(page.locator('#threads')).toHaveClass('catalog extended-large');
  const index = await page.goto('/test/');
  expect(index.headers()['content-security-policy']).toContain("script-src 'none';");
  await expect(page.locator('script')).toHaveCount(0);
  const invalid = await page.goto(`${catalog}?order=invalid`);
  expect(invalid.status()).toBe(400);
  expect(invalid.headers()['content-security-policy']).toContain("script-src 'none';");
});
