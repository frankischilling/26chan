import { test, expect } from '@playwright/test';
test.use({ javaScriptEnabled: true });
for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  test(`${theme} updater controls, countdown, settings and failure status fit desktop and mobile`, async ({ page, context }, info) => {
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ updaterSound: true })));
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 }); await page.goto('/img/thread/1000201');
      const controls = page.locator('.threadNav:visible a[data-cmd="update"]');
      await expect(controls).toHaveCount(2); await expect(controls.first()).toHaveText('Update');
      await expect(page.locator('.threadNav:visible input[data-cmd="sound"]')).toHaveCount(width === 1280 ? 2 : 0);
      const auto = page.locator('.threadNav:visible input[data-cmd="auto"]').first();
      await auto.check(); await expect(page.locator('.threadNav:visible .nativeUpdaterStatus').first()).toHaveText(/^(10|9)$/);
      const countdown = info.outputPath(`${theme}-${width}-countdown.png`);
      await page.locator('.threadNav:visible').first().screenshot({ path: countdown });
      await info.attach(`${theme} ${width} countdown`, { path: countdown, contentType: 'image/png' });
      await auto.uncheck();
      await page.route('**/_watch/img/thread/1000201/posts', route => route.fulfill({ status: 503, body: 'Unavailable' }));
      await controls.first().click();
      await expect(page.locator('.threadNav:visible .nativeUpdaterStatus').first()).toContainText('Connection Error');
      await expect(controls.first()).toBeFocused();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      const path = info.outputPath(`${theme}-${width}-updater.png`);
      await page.locator('.threadNav:visible').first().screenshot({ path });
      await info.attach(`${theme} ${width} updater`, { path, contentType: 'image/png' });
      await page.getByRole('link', { name: 'Settings', exact: true }).click();
      if (!(await page.locator('#setting-threadUpdater').isVisible())) await page.getByRole('button', { name: 'Monitoring', exact: true }).click();
      await expect(page.locator('#setting-threadUpdater')).toBeChecked();
      await expect(page.locator('#setting-alwaysAutoUpdate')).not.toBeChecked();
      await expect(page.locator('#setting-autoScroll')).not.toBeChecked();
      await expect(page.locator('#setting-updaterSound')).toBeChecked();
      const dialog = page.getByRole('dialog', { name: 'Settings', exact: true }), bounds = await dialog.boundingBox();
      expect(bounds.x).toBeGreaterThanOrEqual(0); expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
      expect(bounds.y).toBeGreaterThanOrEqual(0); expect(bounds.y + bounds.height).toBeLessThanOrEqual(900);
      const settings = info.outputPath(`${theme}-${width}-monitoring.png`);
      await dialog.screenshot({ path: settings });
      await info.attach(`${theme} ${width} monitoring`, { path: settings, contentType: 'image/png' });
      await page.keyboard.press('Escape');
    }
  });
}
