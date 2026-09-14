import { test, expect } from '@playwright/test';

for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test(`text-only native form at ${viewport.width}px keeps subject validation and hides uploads`, async ({ page }, info) => {
    await page.setViewportSize(viewport);
    await page.goto('/text-only/');
    await expect(page.locator('body')).toHaveClass('text_only');
    await expect(page.locator('#sub')).toHaveAttribute('required', '');
    await expect(page.locator('#com')).not.toHaveAttribute('required');
    await expect(page.locator('#upfile')).toHaveCount(0);
    await expect(page.locator('#postHelp')).toContainText('Media uploads are unavailable.');
    const validity = await page.locator('#sub').evaluate(input => {
      const empty = input.validity.valueMissing;
      input.value = 'Owned subject-only thread';
      return { empty, filled: input.checkValidity() };
    });
    expect(validity).toEqual({ empty: true, filled: true });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: info.outputPath('text-only.png'), fullPage: true });
  });
}
