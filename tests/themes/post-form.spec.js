import { test, expect } from '@playwright/test';
test.use({ javaScriptEnabled: true });

for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  test(`${theme} native post form controls fit collapsed and expanded desktop/mobile states`, async ({ page, context }, info) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 }); await page.goto('/demo/');
      const desktop = page.locator('#togglePostFormLink'), top = page.locator('#mpostform');
      await expect(width === 1280 ? desktop : top).toBeVisible();
      await expect(width === 1280 ? top : desktop).toBeHidden();
      for (const state of ['collapsed', 'expanded']) {
        if (state === 'expanded') await (width === 1280 ? desktop : top).locator('a').click();
        await expect(page.locator('#postForm')).toBeVisible({ visible: state === 'expanded' });
        if (width === 390 && state === 'expanded') await expect(top.locator('a')).toHaveCSS('color', 'rgb(52, 52, 92)');
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
        const path = info.outputPath(`${theme}-${width}-${state}.png`);
        await page.screenshot({ path }); await info.attach(`${width} ${state}`, { path, contentType: 'image/png' });
      }
    }
  });
}
