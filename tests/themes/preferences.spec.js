import { test, expect } from '@playwright/test';

const themes = [
  ['yotsuba', 'rgb(255, 255, 238)', 'rgb(128, 0, 0)', 'rgb(240, 224, 214)', 'rgb(204, 17, 5)', 'rgb(17, 119, 67)', false],
  ['yotsuba-b', 'rgb(238, 242, 255)', 'rgb(0, 0, 0)', 'rgb(214, 218, 240)', 'rgb(15, 12, 93)', 'rgb(17, 119, 67)', false],
  ['futaba', 'rgb(255, 255, 238)', 'rgb(128, 0, 0)', 'rgb(240, 224, 214)', 'rgb(204, 17, 5)', 'rgb(17, 119, 67)', true],
  ['burichan', 'rgb(238, 242, 255)', 'rgb(0, 0, 0)', 'rgb(214, 218, 240)', 'rgb(15, 12, 93)', 'rgb(17, 119, 67)', true],
  ['photon', 'rgb(238, 238, 238)', 'rgb(51, 51, 51)', 'rgb(221, 221, 221)', 'rgb(17, 17, 17)', 'rgb(0, 74, 153)', false],
  ['tomorrow', 'rgb(29, 31, 33)', 'rgb(197, 200, 198)', 'rgb(40, 42, 46)', 'rgb(178, 148, 187)', 'rgb(197, 200, 198)', false],
];

for (const [device, viewport] of [['desktop', { width: 1280, height: 900 }], ['mobile', { width: 390, height: 844 }]]) {
  test(`six persisted styles without JavaScript on ${device}`, async ({ page, context }) => {
    await page.setViewportSize(viewport);
    await page.goto('/demo/');
    const content = await page.locator('.postMessage').allTextContents();
    for (const [id, paper, ink, panel, subject, name, serif] of themes) {
      await page.getByRole('link', { name: 'Style', exact: true }).click();
      await expect(page.getByLabel('Style', { exact: true })).toBeVisible();
      await page.getByLabel('Style', { exact: true }).selectOption(id);
      await page.getByRole('button', { name: 'Apply style', exact: true }).click();
      await expect(page).toHaveURL('http://127.0.0.1:3000/demo/');
      await expect(page.locator('html')).toHaveCSS('background-color', paper);
      await expect(page.locator('html')).toHaveCSS('color', ink);
      await expect(page.locator('.reply')).toHaveCSS('background-color', panel);
      await expect(page.locator('.subject').first()).toHaveCSS('color', subject);
      await expect(page.locator('.name').first()).toHaveCSS('color', name);
      expect(await page.locator('html').evaluate(node => getComputedStyle(node).fontFamily.includes('Times New Roman'))).toBe(serif);
      expect(await page.locator('.postMessage').allTextContents()).toEqual(content);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      const cookies = await context.cookies();
      expect(cookies).toHaveLength(1);
      expect(cookies[0]).toMatchObject({ name: 'board-theme-ws', value: id, path: '/', httpOnly: true, sameSite: 'Lax' });
      expect(await context.cookies('http://localhost:3004')).toEqual([]);
      await page.reload();
      await expect(page.locator('html')).toHaveCSS('background-color', paper);
      await page.getByRole('link', { name: 'Style', exact: true }).click();
      await expect(page.getByLabel('Style', { exact: true })).toHaveValue(id);
      await page.getByRole('link', { name: 'Return without changing style' }).click();
      const response = await page.request.get('/static/theme.css?worksafe=true', { headers: { 'If-None-Match': '*' } });
      expect(response.status()).toBe(200);
      expect(response.headers()['cache-control']).toBe('private, no-store');
      expect(response.headers().vary).toBe('Cookie');
      await expect(page).toHaveScreenshot(`theme-${id}-${device}.png`, { fullPage: true });
    }
    await page.goto('/settings/theme?worksafe=false');
    await expect(page.getByLabel('Style', { exact: true })).toHaveValue('yotsuba');
    await page.getByLabel('Style', { exact: true }).selectOption('photon');
    await page.getByRole('button', { name: 'Apply style', exact: true }).click();
    await page.goto('/demo/');
    await expect(page.locator('html')).toHaveCSS('background-color', 'rgb(29, 31, 33)');
    await page.getByRole('link', { name: 'Style', exact: true }).click();
    await expect(page.getByLabel('Style', { exact: true })).toHaveValue('tomorrow');
    await page.goto('/settings/theme?worksafe=false');
    await expect(page.getByLabel('Style', { exact: true })).toHaveValue('photon');
    expect((await context.cookies()).map(cookie => [cookie.name, cookie.value]).sort()).toEqual([
      ['board-theme', 'photon'], ['board-theme-ws', 'tomorrow'],
    ]);
  });
}
