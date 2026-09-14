import { test, expect } from '@playwright/test';

const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon'];

for (const theme of themes) {
  test(`text catalog table follows ${theme} desktop and mobile geometry`, async ({ page, context }) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const [name, width] of [['desktop', 1280], ['mobile', 390]]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto('/text-catalog/catalog');
      const catalog = page.locator('#threads'), table = catalog.locator('table');
      await expect(table.locator('tbody > tr')).toHaveCount(6);
      expect(await table.locator('th').allTextContents()).toEqual(['', 'Subject', 'Replies', 'Date', '']);
      expect(await table.locator('.txt-rep i').allTextContents()).toEqual(['2', '3']);
      await expect(table.locator('td.txt-sub').nth(5)).toHaveText('Sticky above bump limit');
      await expect(table.locator('td.txt-sub').nth(4)).toHaveText('<script>literal & subject</script>');
      await expect(table.locator('script, img, .teaser')).toHaveCount(0);
      await expect(table.locator('td.txt-date').first()).toHaveText('09/08/26(Tue)08:00:00');
      for (const id of ['size-ctrl', 'teaser-ctrl', 'theme-nospoiler']) await expect(page.locator(`#${id}`)).toBeHidden();
      await expect(page.locator('#order-ctrl')).toBeVisible();
      await expect(page.locator('#qf-box')).toBeVisible();
      await expect(table.locator('th.txt-date')).toHaveCSS('display', name === 'mobile' ? 'none' : 'table-cell');
      await expect(table.locator('th.txt-ctrl')).toHaveCSS('display', name === 'mobile' ? 'none' : 'table-cell');
      const geometry = await table.evaluate(node => {
        const style = getComputedStyle(node), cell = getComputedStyle(node.querySelector('td'));
        return { ratio: node.getBoundingClientRect().width / node.parentElement.getBoundingClientRect().width,
          collapse: style.borderCollapse, spacing: style.borderSpacing, padding: cell.padding };
      });
      expect(geometry.ratio).toBeCloseTo(name === 'mobile' ? 1 : .8, 2);
      expect(geometry).toMatchObject({ collapse: 'separate', spacing: '2px', padding: '4px 2px' });
      await expect(table.locator('td').first()).toHaveCSS('font-size', ['futaba', 'burichan'].includes(theme) ? '16px' : '13.3333px');
      const bounds = await table.boundingBox(), parent = await catalog.boundingBox();
      expect(bounds.x - parent.x).toBeCloseTo((parent.width - bounds.width) / 2, 1);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      await expect(catalog).toHaveScreenshot(`text-catalog-${theme}-${name}.png`);
    }
  });
}

test.describe('live text rows', () => {
  test.use({ javaScriptEnabled: true });
  test('sort, search, pin deltas, hiding and menus retain the same valid table rows', async ({ page }) => {
    await page.goto('/text-catalog/catalog?order=alt&q=');
    const rows = page.locator('#threads tbody > .thread');
    await expect(rows.first().locator('.postMenuBtn')).toHaveCount(1);
    await page.evaluate(() => {
      window.ownedTextRows = new Map(Array.from(document.querySelectorAll('#threads tbody > .thread'), node => [node.id, node]));
    });
    await page.locator('#order-ctrl').selectOption('r');
    expect(await rows.evaluateAll(nodes => nodes.map(node => node.id))).toEqual([1000403, 1000405, 1000401, 1000400, 1000402, 1000404].map(id => `thread-${id}`));
    const pinned = page.locator('#thread-1000404');
    await pinned.locator('.postMenuBtn').click();
    await expect(page.getByRole('menuitem', { name: 'Report thread' })).toHaveAttribute('href', 'http://127.0.0.1:3000/text-catalog/thread/1000404#report1000404');
    const menu = page.getByRole('menu', { name: 'Thread actions' });
    const bounds = await menu.boundingBox();
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(1280);
    await expect(menu).toHaveScreenshot('text-catalog-menu.png');
    await page.keyboard.press('Escape');
    await expect(pinned.locator('.postMenuBtn')).toBeFocused();
    await pinned.locator('.postMenuBtn').click();
    await page.getByRole('menuitem', { name: 'Pin thread', exact: true }).click();
    await expect(rows.first()).toHaveAttribute('id', 'thread-1000404');
    await expect(pinned).toHaveClass(/\bpinned\b/);
    await expect(pinned.locator('.txt-no')).toHaveCSS('box-shadow', 'rgb(0, 0, 0) -1px 0px 0px 0px');
    await expect(pinned.locator('.txt-rep')).toHaveText('0(+0)');
    await expect(page.locator('.catalogPinPage')).toHaveCount(0);
    await pinned.locator('.postMenuBtn').click();
    await page.getByRole('menuitem', { name: 'Hide thread', exact: true }).click();
    await expect(rows).toHaveCount(5);
    await page.locator('#filters-clear-hidden').click();
    await expect(rows).toHaveCount(1);
    await pinned.locator('.postMenuBtn').click();
    await page.getByRole('menuitem', { name: 'Unhide thread', exact: true }).click();
    await expect(rows).toHaveCount(6);
    await page.locator('#qf-box').fill('Synthetic counter fixture');
    await page.getByRole('button', { name: 'Apply', exact: true }).click();
    await expect(rows).toHaveCount(6);
    await page.locator('#qf-box').fill('^absent-text-catalog$');
    await page.getByRole('button', { name: 'Apply', exact: true }).click();
    await expect(rows).toHaveCount(0);
    await expect(page.locator('#threads > .empty')).toContainText('No matching threads.');
    await page.getByRole('link', { name: 'Show all threads', exact: true }).click();
    await expect(rows).toHaveCount(6);
    expect(await rows.evaluateAll(nodes => nodes.every(node => window.ownedTextRows.get(node.id) === node))).toBe(true);
    expect(await page.locator('#threads tbody').evaluate(node => Array.from(node.children).every(child => child.tagName === 'TR'))).toBe(true);
    await expect(page.locator('#threads .teaser, #threads img, #threads .wbtn')).toHaveCount(0);
    await page.getByRole('button', { name: 'Unpin all threads', exact: true }).click();
    await expect(page.locator('#threads .pinned')).toHaveCount(0);
    await page.evaluate(() => localStorage.setItem('4chan-pin-text-catalog', JSON.stringify({ 1000401: 0 })));
    await page.reload();
    await expect(page.locator('#thread-1000401 .txt-rep')).toHaveText('2 (+2)');
    await expect(page.locator('#threads tbody > tr').first()).toHaveAttribute('id', 'thread-1000401');
    await page.reload();
    await expect(page.locator('#thread-1000401 .txt-rep')).toHaveText('2(+0)');
  });
});
