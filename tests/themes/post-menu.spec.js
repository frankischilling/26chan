import { test, expect } from '@playwright/test';

const themes = {
  yotsuba: { font: 12, right: 2, bottom: 1, panel: 'rgb(240, 224, 214)', edge: 'rgb(217, 191, 183)' },
  'yotsuba-b': { font: 12, right: 2, bottom: 1, panel: 'rgb(214, 218, 240)', edge: 'rgb(183, 197, 217)' },
  futaba: { font: 13, right: 1, bottom: 0, panel: 'rgb(240, 224, 214)', edge: 'rgb(217, 191, 183)' },
  burichan: { font: 13, right: 1, bottom: 0, panel: 'rgb(214, 218, 240)', edge: 'rgb(183, 197, 217)' },
  tomorrow: { font: 12, right: 1, bottom: 0, panel: 'rgb(40, 42, 46)', edge: 'rgb(0, 0, 0)' },
  photon: { font: 12, right: 1, bottom: 0, panel: 'rgb(221, 221, 221)', edge: 'rgb(204, 204, 204)' },
};

test.use({ javaScriptEnabled: true });
for (const [theme, expected] of Object.entries(themes)) {
  test(`${theme} has native desktop/mobile post-menu placement and watcher actions`, async ({ page, context }) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme,
      url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await context.addInitScript(() => {
      localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
      localStorage.setItem('4chan-watch', '{}');
      localStorage.setItem('4chan-tw-timestamp', String(Date.now()));
    });
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto('/demo/');
      await expect(page.locator('.board .wbtn')).toHaveCount(0);
      const trigger = page.getByRole('button', { name: 'Post menu for post 1000001', exact: true });
      await expect(trigger).toHaveText(width === 390 ? '...' : '\u25b6');
      expect(await trigger.evaluate((node, mobile) => mobile
        ? node.parentElement.firstElementChild === node : node.parentElement.lastElementChild === node, width === 390)).toBe(true);
      await trigger.press('ArrowDown');
      await expect(trigger).toHaveAttribute('aria-expanded', 'true');
      const menu = page.locator('#post-menu');
      await expect(menu).toHaveCSS('font-size', `${width === 390 ? 16 : expected.font}px`);
      await expect(menu).toHaveCSS('line-height', width === 390 ? '40px' : `${(expected.font * 1.3).toFixed(1)}px`);
      await expect(menu.locator('ul')).toHaveCSS('background-color', expected.panel);
      await expect(menu.locator('ul')).toHaveCSS('border-top-color', expected.edge);
      await expect(menu.locator('ul')).toHaveCSS('border-right-width', `${expected.right}px`);
      await expect(menu.locator('ul')).toHaveCSS('border-bottom-width', `${expected.bottom}px`);
      await expect(menu.getByRole('menuitem', { name: 'Report post', exact: true })).toBeFocused();
      await expect(menu.getByRole('menuitem', { name: 'Delete post', exact: true })).toHaveCount(width === 390 ? 1 : 0);
      const rect = await menu.boundingBox();
      expect(rect.x).toBeGreaterThanOrEqual(0);
      expect(rect.x + rect.width).toBeLessThanOrEqual(width);
      await menu.getByRole('menuitem', { name: 'Add to watch list', exact: true }).click();
      await expect(page.locator('#watch-1000001-demo')).toHaveCount(1);
      await trigger.click();
      await page.getByRole('menuitem', { name: 'Remove from watch list', exact: true }).click();
      await expect(page.locator('#watch-1000001-demo')).toHaveCount(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    }
  });
}
