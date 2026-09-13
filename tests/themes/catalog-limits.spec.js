import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-catalog-reference.json', import.meta.url), 'utf8'));
const cases = [[1,1,false,false], [2,2,true,true], [1,1,true,false], [3,3,true,true], [0,0,false,false]];

for (const theme of Object.keys(reference.themes)) {
  test(`catalog limit indicators in ${theme}`, async ({ page, context }) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const [name, width, height] of [['desktop',1280,900], ['mobile',390,844]]) {
      await page.setViewportSize({ width, height });
      for (const disabled of [false, true]) {
        await page.goto(disabled ? '/limits/text/catalog' : '/limits/catalog');
        for (const [index, [replies, images, bump, imageLimit]] of cases.entries()) {
          const meta = page.locator(`#meta-${1000400 + index}`);
          await expect(meta).toHaveText(`R: ${replies}${images ? ` / I: ${images}` : ''}`);
          const italic = [];
          if (bump) italic.push(`R: ${replies}`);
          if (imageLimit && !disabled) italic.push(`I: ${images}`);
          expect(await meta.locator('i').allTextContents()).toEqual(italic);
          await expect(meta.locator('b').first()).toHaveCSS('font-style', bump ? 'italic' : 'normal');
          if (images) await expect(meta.locator('b').nth(1)).toHaveCSS('font-style', imageLimit && !disabled ? 'italic' : 'normal');
          await expect(meta).toHaveAttribute('title', '(R)eplies / (I)mage Replies');
        }
        expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
        if (theme === 'yotsuba-b' && !disabled) {
          for (const image of await page.locator('.catalogThumb img').all()) {
            await image.scrollIntoViewIfNeeded();
            await expect.poll(() => image.evaluate(img => img.complete && img.naturalWidth > 0)).toBe(true);
          }
          await page.evaluate(() => scrollTo(0,0));
          await expect.soft(page).toHaveScreenshot(`catalog-limits-${name}.png`, { fullPage: true });
        }
      }
    }
  });
}
