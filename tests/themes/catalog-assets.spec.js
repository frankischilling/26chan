import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const assets = JSON.parse(await readFile(new URL('../../docs/public-catalog-assets.json', import.meta.url), 'utf8'));
const reference = JSON.parse(await readFile(new URL('../../docs/public-catalog-reference.json', import.meta.url), 'utf8'));
for (const theme of Object.keys(reference.themes)) {
  for (const scale of assets.environment.device_scale_factors) {
    test(`catalog state assets in ${theme} at scale ${scale}`, async ({ browser }) => {
      const context = await browser.newContext({ javaScriptEnabled: false, deviceScaleFactor: scale });
      try {
        await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
        const page = await context.newPage();
        const requested = [];
        const loaded = new Set();
        page.on('request', request => requested.push(request.url()));
        page.on('response', response => { if (response.status() === 200) loaded.add(new URL(response.url()).pathname); });
        async function measure(selector) {
          const expected = assets.styles[selector];
          const node = page.locator(selector).first();
          if (selector === '.nofile' || selector === '.imgdel' || selector === '.spoilerImage') {
            await node.scrollIntoViewIfNeeded();
            await expect.poll(() => node.evaluate(img => img.complete && img.naturalWidth > 0)).toBe(true);
          }
          const actual = await node.evaluate((node, keys) => {
            const box = node.getBoundingClientRect(); const style = getComputedStyle(node);
            return { box: [box.width,box.height], css: Object.fromEntries(keys.map(key => [key,style[key]])) };
          }, Object.keys(expected.css));
          expect(actual).toEqual(expected);
        }
        for (const [width, height] of assets.environment.viewports) {
          await page.setViewportSize({ width,height });
          await page.goto('http://127.0.0.1:3000/demo/catalog');
          await measure('.nofile');
          await expect(page.getByRole('img', { name: 'No image', exact: true })).toHaveAttribute('src', '/static/catalog/nofile.png');
          for (const [size, teaser] of [['small','on'],['small','off'],['large','on'],['large','off']]) {
            await page.goto(`http://127.0.0.1:3000/img/catalog?size=${size}&teaser=${teaser}`);
            for (const selector of Object.keys(assets.styles).filter(selector => selector !== '.nofile')) await measure(selector);
            for (const name of ['sticky','closed']) {
              const path = `/static/catalog/${name}${scale === 2 ? '@2x' : ''}.gif`;
              const icon = page.getByRole('img', { name: name === 'sticky' ? 'Sticky' : 'Closed', exact: true });
              await expect(icon).toHaveCSS('background-image', `url("http://127.0.0.1:3000${path}")`);
              await expect(icon).toHaveCSS('background-size', scale === 2 ? '100%' : 'auto');
              await expect.poll(() => loaded.has(path)).toBe(true);
            }
            const link = page.locator('#thread-1000206 .catalogThumb');
            await link.focus();
            await expect(link).toHaveCSS('outline-width', '2px');
            await expect(link).toHaveAttribute('aria-label', 'View thread 1000206');
            await expect(link).toHaveAttribute('href', '/img/thread/1000206');
            expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
          }
        }
        expect(requested.some(url => new URL(url).port === '3004' && /100020[56]/.test(url))).toBe(false);
        expect(requested.some(url => new URL(url).hostname === 's.4cdn.org')).toBe(false);
      } finally { await context.close(); }
    });
  }
}
