import { test, expect } from '../helpers/visual-diagnostics.js';
import { readFile } from 'node:fs/promises';
import { screenshotPixel } from '../../scripts/viewport-pixels.mjs';

const reference = JSON.parse(await readFile(new URL('../../docs/public-viewport-reference.json', import.meta.url)));
const origin = 'http://127.0.0.1:3000';
for (const row of reference.cases) {
  test.describe(`${row.theme} ${row.mode} ws=${row.worksafe} ${row.width} DPR=${row.scale} optout=${row.neverMobile} dark=${row.dark}`, () => {
    test.use({ javaScriptEnabled: true, viewport: { width: row.width, height: reference.height }, deviceScaleFactor: row.scale });
    test('the short public page paints the recorded viewport below its body', async ({ page, context, visualDiagnostics }) => {
      const errors = [], external = [];
      page.on('pageerror', error => errors.push(error.message));
      page.on('request', request => { if (new URL(request.url()).origin !== origin) external.push(request.url()); });
      await context.addCookies([{ name: row.worksafe ? 'board-theme-ws' : 'board-theme', value: row.theme, url: origin }]);
      await context.addInitScript(neverMobile => { localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })); if (neverMobile) localStorage.setItem('4chan_never_show_mobile', 'true'); }, row.neverMobile);
      await page.goto(`/chrome/${row.worksafe ? 'demo' : 'zed'}/${row.mode === 'catalog' ? 'catalog' : ''}`);
      await expect(page.locator('body')).toHaveAttribute('data-native-never-mobile', String(row.neverMobile));
      expect(visualDiagnostics.failedStylesheets, 'Stylesheet transport must succeed before comparing viewport pixels').toEqual([]);
      await page.evaluate(dark => document.body.classList.toggle('m-dark', dark), row.dark);
      const actual = await page.evaluate(() => {
        const computed = getComputedStyle(document.documentElement);
        return { root: Object.fromEntries(['backgroundColor', 'backgroundImage', 'backgroundRepeat', 'backgroundPosition', 'backgroundSize'].map(key => [key, computed[key]])), bodyBottom: document.body.getBoundingClientRect().bottom };
      });
      expect(actual.bodyBottom).toBeLessThan(row.point.y - 1);
      const pixel = screenshotPixel(await page.screenshot(), row.point, row.scale);
      expect({ root: actual.root, pixel }).toEqual({ root: row.root, pixel: row.pixel });
      expect(errors).toEqual([]); expect(external).toEqual([]);
    });
  });
}
