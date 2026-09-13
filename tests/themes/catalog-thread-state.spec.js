import { test, expect } from '@playwright/test';

test.use({ javaScriptEnabled: true });

const themes = {
  yotsuba: ['12px', 'rgb(240, 224, 214)', 'rgb(217, 191, 183)', 'rgb(0, 0, 128)'],
  'yotsuba-b': ['13px', 'rgb(214, 218, 240)', 'rgb(183, 197, 217)', 'rgb(0, 0, 128)'],
  futaba: ['13px', 'rgb(240, 224, 214)', 'rgb(217, 191, 183)', 'rgb(0, 0, 128)'],
  burichan: ['13px', 'rgb(214, 218, 240)', 'rgb(183, 197, 217)', 'rgb(0, 0, 128)'],
  tomorrow: ['12px', 'rgb(40, 42, 46)', 'rgb(0, 0, 0)', 'rgb(95, 137, 172)'],
  photon: ['12px', 'rgb(221, 221, 221)', 'rgb(204, 204, 204)', 'rgb(255, 102, 0)'],
};

for (const [theme, expected] of Object.entries(themes)) {
  test(`catalog thread menus and pins follow ${theme} reference colors on desktop and mobile`, async ({ page, context }) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const [name, width, height] of [['desktop', 1280, 900], ['mobile', 390, 844]]) {
      await page.setViewportSize({ width, height });
      await page.goto('/img/catalog?order=alt&size=small&teaser=on&q=');
      const card = page.locator('#threads > .thread').first();
      await card.hover();
      const button = card.locator('.postMenuBtn');
      await button.click();
      const menu = page.getByRole('menu', { name: 'Thread actions' });
      await expect(menu).toBeVisible();
      const style = await menu.evaluate(node => {
        const menu = getComputedStyle(node);
        const list = getComputedStyle(node.querySelector('ul'));
        const button = getComputedStyle(node.closest('.meta').querySelector('.postMenuBtn'));
        return [menu.fontSize, list.backgroundColor, list.borderTopColor, button.color];
      });
      expect(style).toEqual(expected);
      const bounds = await menu.boundingBox();
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
      await expect(menu).toHaveScreenshot(`catalog-menu-${theme}-${name}.png`);
      await page.getByRole('menuitem', { name: 'Pin thread', exact: true }).click();
      const pinned = page.locator('#threads .thumb.pinned');
      await expect(pinned).toHaveCount(1);
      expect(await pinned.evaluate(node => { const style = getComputedStyle(node); return [style.borderTopWidth, style.borderTopStyle]; })).toEqual(['3px', 'dashed']);
      await expect(page.locator('.catalogPinPage:visible')).toContainText('P:');
      await page.getByRole('button', { name: 'Unpin all threads', exact: true }).click();
    }
  });
}

for (const theme of Object.keys(themes)) {
test(`pin styling covers real image, spoiler, deleted and no-file cards in ${theme} without changing their sources or dimensions`, async ({ page, context }) => {
  await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
  for (const [width, height] of [[1280, 900], [390, 844]]) {
    await page.setViewportSize({ width, height });
    const coveredClasses = [];
    for (const route of ['/img/catalog?order=alt&size=small&teaser=on&q=', '/demo/catalog?order=alt&size=small&teaser=on&q=']) {
    await page.goto(route);
    const cards = await page.locator('#threads > .thread').evaluateAll(nodes => nodes.map(node => {
      const image = node.querySelector('.catalogThumb img');
      return { id: node.id, classes: image.className, source: image.getAttribute('src'), width: image.getAttribute('width'), height: image.getAttribute('height') };
    }));
    expect(cards.length).toBeGreaterThan(0);
    coveredClasses.push(...cards.flatMap(card => card.classes.split(' ')));
    await page.evaluate(() => { window.originalCatalogCards = new Map(Array.from(document.querySelectorAll('#threads > .thread'), node => [node.id, node])); });
    for (const entry of cards) {
      const card = page.locator(`#${entry.id}`);
      await card.hover();
      await card.locator('.postMenuBtn').click();
      await page.getByRole('menuitem', { name: 'Pin thread', exact: true }).click();
      const image = card.locator('.catalogThumb img');
      await expect(image).toHaveClass(/\bpinned\b/);
      for (const side of ['top', 'right', 'bottom', 'left']) {
        await expect(image).toHaveCSS(`border-${side}-width`, '3px');
        await expect(image).toHaveCSS(`border-${side}-style`, 'dashed');
      }
      expect(await image.getAttribute('src')).toBe(entry.source);
      expect(await image.getAttribute('width')).toBe(entry.width);
      expect(await image.getAttribute('height')).toBe(entry.height);
      expect(await card.evaluate(node => window.originalCatalogCards.get(node.id) === node)).toBe(true);
      await page.getByRole('button', { name: 'Unpin all threads', exact: true }).click();
      await expect(image).not.toHaveClass(/\bpinned\b/);
    }
    }
    expect(coveredClasses).toEqual(expect.arrayContaining(['spoilerImage', 'imgdel', 'nofile']));
  }
});
}
