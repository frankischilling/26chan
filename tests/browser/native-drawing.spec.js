import { test, expect } from '@playwright/test';

// This persisted public lane has media disabled. Actual drawings and approvals
// are separately qualified by drawing-upload.mjs in the isolated media lane.
test('drawing remains gated and lazy on the media-disabled public service', async ({ page }) => {
  const fetched = [];
  page.on('request', request => { if (request.url().includes('/static/tegaki/')) fetched.push(request.url()); });
  for (const board of ['demo', 'qst', 'vip', 'i']) {
    const response = await page.goto(`/${board}/`);
    expect(response.status()).toBe(200);
    await expect(page.locator('form.postEditor[data-drawing-allowed="true"]')).toHaveCount(0);
    await expect(page.locator('[data-drawing-draw], #qr-painter-ctrl, #tegaki')).toHaveCount(0);
  }
  expect(fetched).toEqual([]);
});

test('disabled board mobile and catalog never load or expose a drawing editor', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const modules = [];
  page.on('request', request => { if (/tegaki-0\.9\.4\.v1\.js$/.test(request.url())) modules.push(request.url()); });
  for (const path of ['/demo/', '/qst/catalog', '/vip/catalog']) {
    const response = await page.goto(path); expect(response.status()).toBe(200);
    await expect(page.locator('[data-drawing-draw]:visible, #tegaki')).toHaveCount(0);
  }
  expect(modules).toEqual([]);
});

test('pinned editor assets are local, inert until imported, and contain their license', async ({ request }) => {
  const script = await request.get('/static/tegaki/tegaki-0.9.4.v1.js');
  expect(script.status()).toBe(200);
  const text = await script.text();
  expect(text).toContain('VERSION:"0.9.4"'); expect(text).toContain('MIT License');
  expect(text).toMatch(/export\s*\{\s*Tegaki\s*\}/);
  expect(text).not.toMatch(/window\.Tegaki\s*=|globalThis\.Tegaki\s*=/);
  const sheet = await request.get('/static/tegaki/tegaki-0.9.4.v1.css');
  expect(sheet.status()).toBe(200);
  expect(await sheet.text()).toContain("url('./tegaki-icons.v1.woff')");
  const font = await request.get('/static/tegaki/tegaki-icons.v1.woff');
  expect(font.status()).toBe(200); expect((await font.body()).subarray(0, 4).toString()).toBe('wOFF');
});
