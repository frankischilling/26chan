import { test, expect } from '../helpers/visual-diagnostics.js';

// Pinned sources: imgboard.php rebuild_archive_list; #arc-list, .postblock and
// table.flashListing in yotsubanew/yotsubluenew/futabanew/burichannew/photon/tomorrow.css.
// These are source values, not values recorded from the replacement renderer.
const themes = [
  { id: 'yotsuba', stripe: 'rgb(237, 226, 212)', label: 'rgb(238, 170, 136)', ink: 'rgb(136, 0, 0)', border: '1px', size: '12px' },
  { id: 'yotsuba-b', stripe: 'rgb(224, 229, 246)', label: 'rgb(153, 136, 238)', ink: 'rgb(0, 0, 0)', border: '1px', size: '12px' },
  { id: 'futaba', stripe: 'rgb(237, 226, 212)', label: 'rgb(238, 170, 136)', ink: 'rgb(136, 0, 0)', border: '0px', size: '16px' },
  { id: 'burichan', stripe: 'rgb(224, 229, 246)', label: 'rgb(153, 136, 238)', ink: 'rgb(0, 0, 0)', border: '0px', size: '16px' },
  { id: 'photon', stripe: 'rgb(136, 136, 136)', label: 'rgb(221, 221, 221)', ink: 'rgb(51, 51, 51)', border: '0px', size: '12px' },
  { id: 'tomorrow', stripe: 'rgba(255, 255, 255, 0.1)', label: 'rgb(40, 42, 46)', ink: 'rgb(197, 200, 198)', border: '0px', size: '12px' },
];
for (const theme of themes) {
  for (const width of [1280, 390]) {
    test(`${theme.id} archive table retains source styles and semantics at ${width}px`, async ({ page, context }) => {
      await page.setViewportSize({ width, height: 900 });
      await context.addCookies([{ name: 'board-theme-ws', value: theme.id, url: 'http://127.0.0.1:3000' }]);
      await page.goto('/arc/archive');
      const table = page.locator('table#arc-list.flashListing');
      const heading = page.getByRole('heading', { name: 'Displaying 3 expired threads from the past 3 days', exact: true });
      await expect(heading).toBeVisible();
      await expect(heading).toHaveCSS('text-align', 'center');
      await expect(table).toHaveCSS('max-width', '80%');
      await expect(table).toHaveCSS('margin-top', '10px');
      await expect(table).toHaveCSS('border-spacing', '2px');
      await expect(table.locator('thead td')).toHaveText(['No.', 'Excerpt', '']);
      const header = table.locator('thead td').first();
      await expect(header).toHaveCSS('padding', '5px');
      await expect(header).toHaveCSS('background-color', theme.label);
      await expect(header).toHaveCSS('color', theme.ink);
      await expect(header).toHaveCSS('border-top-width', theme.border);
      await expect(header).toHaveCSS('font-weight', '700');
      await expect(header).toHaveCSS('font-size', theme.size);
      const rows = table.locator('tbody tr');
      await expect(rows).toHaveCount(3);
      await expect(rows.locator('td:first-child')).toHaveText(['1000101', '1000102', '1000103']);
      await expect(rows.nth(0)).toHaveCSS('background-color', theme.stripe);
      await expect(rows.nth(1)).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
      await expect(rows.nth(2)).toHaveCSS('background-color', theme.stripe);
      for (let index = 0; index < 3; index++) {
        const row = rows.nth(index);
        await expect(row.locator('td')).toHaveCount(3);
        const number = row.locator('td').first();
        await expect(number).toHaveCSS('padding', '2px');
        await expect(number).toHaveCSS('text-align', 'center');
        await expect(number).toHaveCSS('font-size', theme.size);
        const excerpt = row.locator('.teaser-col');
        await expect(excerpt).toBeVisible();
        await expect(excerpt).toHaveCSS('text-align', 'left');
        await expect(excerpt).toHaveCSS('word-break', 'break-all');
        const link = row.getByRole('link', { name: 'View', exact: true });
        await expect(link).toBeVisible();
        await expect(link).toHaveAttribute('href', new RegExp(`^/arc/thread/${1000101 + index}(?:/[a-z0-9-]+)?$`));
        await expect(link).toHaveCSS('color', 'rgb(52, 52, 92)');
        await expect(link).toHaveCSS('text-decoration-line', 'underline');
      }
      const geometry = await table.evaluate(element => {
        const box = element.getBoundingClientRect(), parent = element.parentElement.getBoundingClientRect();
        return { left: box.left, right: box.right, center: box.x + box.width / 2,
          parentCenter: parent.x + parent.width / 2, width: box.width, parentWidth: parent.width,
          overflow: document.documentElement.scrollWidth > innerWidth };
      });
      expect(geometry.overflow).toBe(false);
      expect(geometry.width).toBeLessThanOrEqual(geometry.parentWidth * 0.8 + 1);
      expect(Math.abs(geometry.center - geometry.parentCenter)).toBeLessThanOrEqual(1);
      expect(geometry.left).toBeGreaterThanOrEqual(0);
      expect(geometry.right).toBeLessThanOrEqual(width);
      const view = rows.first().getByRole('link', { name: 'View', exact: true });
      await view.hover();
      await expect(view).toHaveCSS('color', 'rgb(221, 0, 0)');
      await view.click();
      await expect(page.getByText('This thread is archived and read-only.', { exact: true })).toBeVisible();
      await expect(page.locator('#postForm')).toHaveCount(0);
      await page.goto('/emptyarc/archive');
      await expect(page.getByRole('heading', { name: 'Displaying 0 expired threads from the past 3 days', exact: true })).toBeVisible();
      await expect(table).toBeVisible();
      await expect(table.locator('thead td')).toHaveText(['No.', 'Excerpt', '']);
      await expect(table.locator('tbody tr')).toHaveCount(0);
      await expect(table.locator('a')).toHaveCount(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    });
  }
}
