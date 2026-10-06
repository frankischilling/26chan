import { watcherSettingsOpener } from '../browser/helpers/watcher-settings.js';
import { test, expect } from '../helpers/visual-diagnostics.js';

test.use({ javaScriptEnabled: true });
for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  test(`${theme} desktop shortcut help restores focus and mobile hides controls without disabling saved shortcuts`, async ({ page, context }, info) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme,
      url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true })));
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 }); await page.goto('/demo/');
      await watcherSettingsOpener(page).click();
      await page.getByRole('button', { name: 'Navigation', exact: true }).click();
      if (width === 390) {
        await expect(page.locator('#setting-keyBinds, #keybinds-open')).toHaveCount(0);
        const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
        const expansion = dialog.locator('#setting-threadExpansion');
        const original = await expansion.isChecked();
        await expansion.setChecked(!original);
        const bounds = await dialog.boundingBox();
        expect(bounds.x).toBeGreaterThanOrEqual(0); expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
        expect(bounds.y).toBeGreaterThanOrEqual(0); expect(bounds.y + bounds.height).toBeLessThanOrEqual(900);
        const path = info.outputPath(`${theme}-${width}-navigation.png`);
        await dialog.screenshot({ path, animations: 'disabled' });
        await info.attach(`${theme} ${width} mobile navigation`, { path, contentType: 'image/png' });
        await page.keyboard.press('Escape');
        await watcherSettingsOpener(page).click();
        await page.getByRole('button', { name: 'Navigation', exact: true }).click();
        await expect(expansion).toBeChecked({ checked: original });
        await page.keyboard.press('Escape');
        expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).keyBinds)).toBe(true);
        expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
        await page.getByRole('heading', { level: 1 }).click();
        await Promise.all([page.waitForURL('**/demo/catalog'), page.keyboard.press('c')]);
        continue;
      }
      await expect(page.locator('#setting-keyBinds')).toBeChecked();
      await page.getByRole('link', { name: 'Show', exact: true }).click();
      const help = page.getByRole('dialog', { name: 'Keyboard Shortcuts', exact: true });
      await expect(help.locator('kbd')).toHaveText(['A', 'Q', 'R', 'W', 'B', 'N', 'I', 'C', 'F', 'Ctrl + Click', 'Ctrl + S', 'Esc']);
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
