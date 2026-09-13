import { test, expect } from '@playwright/test';

// Synthetic template fixtures plus real public error handling, not reference parity.
for (const [name, viewport] of [
  ['desktop', { width: 1280, height: 900 }],
  ['mobile', { width: 390, height: 844 }],
]) {
  for (const catalog of [false, true]) {
    test(`empty ${catalog ? 'catalog' : 'board'} ${name}`, async ({ page }) => {
      await page.setViewportSize(viewport);
      const response = await page.goto(`/empty/${catalog ? 'catalog' : ''}`);
      expect(response.status()).toBe(200);
      await expect(page.locator('.thread')).toHaveCount(0);
      await expect(page.locator('#postForm')).toHaveCount(catalog ? 0 : 1);
      await expect(page.locator('.empty')).toContainText('No threads yet.');
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
      await expect(page).toHaveScreenshot(`empty-${catalog ? 'catalog' : 'board'}-${name}.png`, { fullPage: true });
      if (catalog) {
        const start = page.getByRole('link', { name: 'Start the first thread', exact: true });
        await expect(start).toHaveAttribute('href', '/empty/#postForm');
        await start.click();
        await expect(page).toHaveURL('http://127.0.0.1:3000/empty/#postForm');
      }
      await expect(page.getByRole('heading', { name: 'Start a new thread' })).toBeVisible();
      await page.getByLabel('Comment', { exact: true }).fill('Synthetic first thread draft');
      await expect(page.getByRole('button', { name: 'Post', exact: true })).toBeEnabled();
    });
  }

  test(`empty board directory ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const response = await page.goto('/');
    expect(response.status()).toBe(200);
    await expect(page.getByText('No boards are available yet.', { exact: true })).toBeVisible();
    await expect(page.locator('.boardDirectory article')).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
    await expect(page).toHaveScreenshot(`empty-directory-${name}.png`, { fullPage: true });
  });

  for (const [state, path, status, message] of [
    ['not-found', '/missing/route/extra', 404, 'Page not found.'],
    ['storage-unavailable', '/offline/', 503, 'Storage is unavailable. Try again later.'],
  ]) {
    test(`${state} ${name}`, async ({ page }) => {
      await page.setViewportSize(viewport);
      const response = await page.goto(path);
      expect(response.status()).toBe(status);
      expect(response.headers()['content-security-policy']).toContain("script-src 'none'");
      expect(response.headers()['x-content-type-options']).toBe('nosniff');
      await expect(page.getByRole('heading', { name: 'Request could not be completed' })).toBeVisible();
      await expect(page.getByText(message, { exact: true })).toBeVisible();
      await expect(page.locator('form')).toHaveCount(0);
      await expect(page.locator('body')).not.toContainText('postgres://');
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
      await expect(page).toHaveScreenshot(`${state}-${name}.png`, { fullPage: true });
      await page.getByRole('link', { name: 'All boards', exact: true }).click();
      await expect(page.getByText('No boards are available yet.', { exact: true })).toBeVisible();
    });
  }
}
