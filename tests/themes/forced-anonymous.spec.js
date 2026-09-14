import { test, expect } from '@playwright/test';

for (const width of [1280, 390]) {
  test(`forced-anonymous source form hides identity fields at ${width}px`, async ({ page }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/forced-anonymous/');
    await expect(page.locator('#name, #sub')).toHaveCount(0);
    await expect(page.locator('form.postEditor input[name=name]')).toHaveAttribute('type', 'hidden');
    await expect(page.locator('#email')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Post', exact: true })).toBeVisible();
    await expect(page.locator('#com')).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: info.outputPath('forced-anonymous.png'), fullPage: true });
  });
}
