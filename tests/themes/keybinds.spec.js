import { test, expect } from '@playwright/test';

test.use({ javaScriptEnabled: true });
for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  test(`${theme} shortcut settings and help fit desktop and mobile and restore focus`, async ({ page, context }, info) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme,
      url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true })));
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 }); await page.goto('/demo/');
      await page.getByRole('link', { name: 'Settings', exact: true }).click();
      await page.getByRole('button', { name: 'Navigation', exact: true }).click();
      await expect(page.locator('#setting-keyBinds')).toBeChecked();
      await page.getByRole('link', { name: 'Show', exact: true }).click();
      const help = page.getByRole('dialog', { name: 'Keyboard Shortcuts', exact: true });
      await expect(help.locator('kbd')).toHaveText(['W', 'B', 'N', 'I', 'C', 'F']);
      await expect(page.getByRole('button', { name: 'Close keyboard shortcuts', exact: true })).toBeFocused();
      const box = await help.boundingBox();
      expect(box.x).toBeGreaterThanOrEqual(0); expect(box.x + box.width).toBeLessThanOrEqual(width);
      expect(box.y).toBeGreaterThanOrEqual(0); expect(box.y + box.height).toBeLessThanOrEqual(900);
      const path = info.outputPath(`${theme}-${width}-shortcuts.png`);
      await help.screenshot({ path, animations: 'disabled' });
      await info.attach(`${theme} ${width} shortcut help`, { path, contentType: 'image/png' });
      await page.keyboard.press('Escape');
      await expect(page.getByRole('link', { name: 'Show', exact: true })).toBeFocused();
      await page.keyboard.press('Escape');
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    }
  });
}
