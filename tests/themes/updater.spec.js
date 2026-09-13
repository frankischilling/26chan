import { test, expect } from '@playwright/test';
test.use({ javaScriptEnabled: true });
for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  test(`${theme} manual updater controls and failure status fit desktop and mobile`, async ({ page, context }, info) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 }); await page.goto('/img/thread/1000201');
      const controls = page.locator('.threadNav:visible a[data-cmd="update"]');
      await expect(controls).toHaveCount(2); await expect(controls.first()).toHaveText('Update');
      await page.route('**/_watch/img/thread/1000201/posts', route => route.fulfill({ status: 503, body: 'Unavailable' }));
      await controls.first().click();
      await expect(page.locator('.threadNav:visible .nativeUpdaterStatus').first()).toContainText('Connection Error');
      await expect(controls.first()).toBeFocused();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      const path = info.outputPath(`${theme}-${width}-updater.png`);
      await page.locator('.threadNav:visible').first().screenshot({ path });
      await info.attach(`${theme} ${width} updater`, { path, contentType: 'image/png' });
    }
  });
}
