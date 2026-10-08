import { test, expect } from '@playwright/test';

// Deterministic project baselines exercise the production Askama templates.
// Real persistence and rollover are covered by tests/browser/archive.spec.js.
for (const [name, viewport] of [
  ['desktop', { width: 1280, height: 900 }],
  ['mobile', { width: 390, height: 844 }],
]) {
  test(`populated archive ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto('/arc/archive');
    await expect(page.getByRole('heading', { name: 'Displaying 3 expired threads from the past 3 days', exact: true })).toBeVisible();
    const table = page.locator('table#arc-list.flashListing');
    await expect(table).toBeVisible();
    await expect(table.locator('thead td')).toHaveText(['No.', 'Excerpt', '']);
    await expect(table.locator('tbody tr')).toHaveCount(3);
    await expect(table.locator('tbody tr > td:first-child')).toHaveText(['1000101', '1000102', '1000103']);
    await expect(table.locator('.teaser-col').first()).toContainText('<b>A paper lighthouse</b>');
    await expect(table.locator('.teaser-col b b')).toHaveCount(0);
    await expect(table.getByRole('link', { name: 'View', exact: true })).toHaveCount(3);
    for (const [index, id] of ['1000101', '1000102', '1000103'].entries()) {
      await expect(table.locator('tbody tr').nth(index).locator('td')).toHaveCount(3);
      await expect(table.locator('tbody tr').nth(index).getByRole('link', { name: 'View', exact: true }))
        .toHaveAttribute('href', new RegExp(`^/arc/thread/${id}(?:/[a-z0-9-]+)?$`));
    }
    await expect(table.locator('time')).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
    await expect(page).toHaveScreenshot(`archive-populated-${name}.png`, { fullPage: true });
  });

  test(`empty archive ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto('/emptyarc/archive');
    await expect(page.getByRole('heading', { name: 'Displaying 0 expired threads from the past 3 days', exact: true })).toBeVisible();
    await expect(page.locator('table#arc-list.flashListing')).toBeVisible();
    await expect(page.locator('#arc-list thead td')).toHaveText(['No.', 'Excerpt', '']);
    await expect(page.locator('#arc-list tbody tr')).toHaveCount(0);
    await expect(page.locator('#arc-list a')).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
    await expect(page).toHaveScreenshot(`archive-empty-${name}.png`, { fullPage: true });
  });

  test(`archived thread ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto('/arc/thread/1000101');
    await expect(page.getByText('This thread is archived and read-only.', { exact: true })).toBeVisible();
    await expect(page.locator('#postForm')).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Return', exact: true }).first()).toBeVisible();
    await page.locator('#p1000101 summary').click();
    const ownership = page.locator('#delete1000101');
    await expect(ownership).toHaveAttribute('type', 'hidden');
    await expect(ownership).toHaveAttribute('name', 'password');
    await expect(ownership).toHaveValue('');
    await expect(ownership).toBeHidden();
    await expect(page.getByRole('button', { name: 'Delete post', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Report post', exact: true })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
    await expect(page).toHaveScreenshot(`archived-thread-${name}.png`, { fullPage: true });
  });
}
