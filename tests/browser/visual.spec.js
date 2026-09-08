import { test, expect } from '@playwright/test';

// These are project regression baselines, not evidence of visual reference parity.
for (const [name, viewport] of [['desktop', { width: 1280, height: 900 }], ['mobile', { width: 390, height: 844 }]]) {
  test(`synthetic board ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto('/demo/');
    await expect(page.locator('#m1000001')).toBeVisible();
    await expect(page).toHaveScreenshot(`board-${name}.png`, { fullPage: true });
  });
}
test('synthetic catalog', async ({ page }) => {
  await page.goto('/demo/catalog');
  await expect(page).toHaveScreenshot('catalog-desktop.png', { fullPage: true });
});
