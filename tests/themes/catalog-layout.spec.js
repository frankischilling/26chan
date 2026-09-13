import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-catalog-reference.json', import.meta.url), 'utf8'));
for (const theme of Object.keys(reference.themes)) {
  test(`referenced catalog cards in ${theme}`, async ({ page, context }) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const [width, height] of reference.environment.viewports) {
      await page.setViewportSize({ width, height });
      await page.goto('/img/catalog');
      const card = page.locator('.catalog .thread').first();
      const actual = await card.evaluate((node, keys) => {
        const style = getComputedStyle(node);
        return Object.fromEntries(keys.map(key => [key, style[key]]));
      }, Object.keys(reference.common));
      expect(actual).toEqual(reference.common);
      await expect(card).toHaveCSS('width', width <= 480 ? '155px' : '180px');
      await expect(card.locator('.meta')).toHaveCSS('font-size', '11px');
      await expect(card.locator('.meta')).toHaveCSS('line-height', '8px');
      await expect(card.locator('.meta')).toHaveCSS('margin', '2px 0px 1px');
      await expect(card.locator('.teaser')).toHaveCSS('padding', '0px 15px');
      await expect(card.locator('.thumb')).toHaveCSS('display', 'inline');
      await expect(card.locator('.thumb')).toHaveCSS('min-width', '50px');
      await expect(card.locator('.thumb')).toHaveCSS('min-height', '50px');
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      for (const id of ['order-ctrl','size-ctrl','teaser-ctrl','qf-box']) {
        const label = await page.locator(`label[for="${id}"]`).boundingBox();
        const field = await page.locator(`#${id}`).boundingBox();
        expect(label.y + label.height / 2).toBeCloseTo(field.y + field.height / 2, 0);
      }
      await card.locator('.catalogThumb').focus();
      await expect(card.locator('.catalogThumb')).toHaveCSS('outline-width', '2px');
      await expect(card).toHaveCSS('max-height', 'none');
      await card.locator('.catalogThumb').press('Enter');
      await expect(page).toHaveURL(/\/img\/thread\/1000201$/);
      await expect(page.locator('#p1000201')).toBeVisible();
      for (const [size, teaser, cardWidth, maxHeight] of [['small','off',width <= 480 ? '155px' : '165px','none'], ['large','off','270px','none'], ['large','on','270px','410px']]) {
        await page.goto(`/img/catalog?size=${size}&teaser=${teaser}`);
        const card = page.locator('.catalog .thread').first();
        await expect(card).toHaveCSS('width', cardWidth);
        await expect(card).toHaveCSS('max-height', maxHeight);
        await expect(card.locator('.teaser')).toHaveCount(teaser === 'on' ? 1 : 0);
        await expect(card.locator('.thumb')).toHaveAttribute('width', size === 'large' ? '250' : '150');
        await expect(page.locator('#size-ctrl')).toHaveValue(size);
        await expect(page.locator('#teaser-ctrl')).toHaveValue(teaser);
        expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
        await card.locator('.catalogThumb').focus();
        await expect(card).toHaveCSS('max-height', 'none');
      }
    }
  });
}
