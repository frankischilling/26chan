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
    await expect(page.locator('.archiveEntries li')).toHaveCount(3);
    await expect(page.getByRole('link', { name: 'No.1000101 — <b>A paper lighthouse</b>', exact: true })).toBeVisible();
    await expect(page.locator('.archiveEntries b')).toHaveCount(0);
    await expect(page.locator('.archiveEntries time')).toHaveCount(3);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
    await expect(page).toHaveScreenshot(`archive-populated-${name}.png`, { fullPage: true });
  });

  test(`empty archive ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto('/emptyarc/archive');
    await expect(page.getByText('No archived threads.', { exact: true })).toBeVisible();
    await expect(page.locator('.archiveEntries a')).toHaveCount(0);
    await expect(page).toHaveScreenshot(`archive-empty-${name}.png`, { fullPage: true });
  });

  test(`archived thread ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto('/arc/thread/1000101');
    await expect(page.getByText('This thread is archived and read-only.', { exact: true })).toBeVisible();
    await expect(page.locator('#postForm')).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Archive', exact: true })).toBeVisible();
    await page.locator('#p1000101 summary').click();
    await expect(page.getByRole('button', { name: 'Delete post', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Report post', exact: true })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
    await expect(page).toHaveScreenshot(`archived-thread-${name}.png`, { fullPage: true });
  });
}
