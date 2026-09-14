import { test, expect } from '@playwright/test';

// Project captures document the implemented states. They are not, by themselves,
// evidence of pixel parity with a captured public reference page.
const themes = { yotsuba: 'futaba', 'yotsuba-b': 'burichan', futaba: 'futaba',
  burichan: 'burichan', tomorrow: 'tomorrow', photon: 'photon' };

for (const [theme, family] of Object.entries(themes)) {
  for (const density of [1, 2]) {
    test.describe(`${theme} thread hiding at ${density}x`, () => {
      test.use({ javaScriptEnabled: true, deviceScaleFactor: density });
      test('desktop and mobile restoration controls use the native theme assets', async ({ page, context }, info) => {
        await context.addCookies([{ name: 'board-theme-ws', value: theme,
          url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
        await context.addInitScript(() => {
          localStorage.setItem('4chan-settings', '{}');
          localStorage.removeItem('4chan-hide-t-demo');
          localStorage.setItem('4chan-purge-t-demo', String(Date.now()));
        });
        for (const width of [1280, 390]) {
          await page.setViewportSize({ width, height: 900 });
          await page.goto('/demo/');
          const control = page.locator('#sa1000001');
          if (width === 1280) {
            await expect(control.locator('img')).toHaveAttribute('src',
              `/static/watcher/${family}/post_expand_minus${density === 2 ? '@2x' : ''}.png`);
            await expect(control.locator('img')).toHaveCSS('width', '18px');
            await expect(control.locator('img')).toHaveCSS('height', '18px');
            await control.focus(); await control.press('Enter');
            await expect(control.locator('img')).toHaveAttribute('src',
              `/static/watcher/${family}/post_expand_plus${density === 2 ? '@2x' : ''}.png`);
            await expect(page.locator('#pi1000001')).toBeVisible();
            await expect(page.locator('#m1000001')).toBeHidden();
          } else {
            await page.getByRole('button', { name: 'Post menu for post 1000001', exact: true }).click();
            await page.getByRole('menuitem', { name: 'Hide thread', exact: true }).click();
            await expect(page.locator('#t1000001')).toBeHidden();
            await expect(control).toHaveText('Show Hidden Thread');
            await expect(control).toBeFocused();
            await expect(control).toHaveCSS('box-sizing', 'content-box');
            await expect(control).toHaveCSS('width', '150px');
            await expect(control).toHaveCSS('font-weight', '700');
            await expect(control).toHaveCSS('color', 'rgb(52, 52, 92)');
            await expect(control).toHaveCSS('background-image', 'url("http://127.0.0.1:3000/static/watcher/buttonfade-blue.png")');
            expect(await control.evaluate(element => {
              const range = document.createRange(); range.selectNodeContents(element);
              return range.getClientRects().length;
            })).toBe(1);
          }
          const box = await control.boundingBox();
          expect(box.x).toBeGreaterThanOrEqual(0);
          expect(box.x + box.width).toBeLessThanOrEqual(width);
          if (width === 390) expect(box.width).toBe(172);
          const path = info.outputPath(`${theme}-${density}x-${width}-hidden.png`);
          await page.screenshot({ path, fullPage: true, animations: 'disabled' });
          await info.attach(`${theme} ${density}x ${width} hidden`, { path, contentType: 'image/png' });
          await control.press('Enter');
          await expect(page.locator('#m1000001')).toBeVisible();
          expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
        }
      });
    });
  }
}
