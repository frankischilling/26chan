import { test, expect } from '@playwright/test';
import { withOwnedBlotter } from './helpers/blotter-fixture.js';

test('operator publications render, dismiss, reappear and paginate through real public pages', async ({ page }) => {
  test.setTimeout(120000);
  await withOwnedBlotter(async ({ slug, disabled, publish }) => {
    await page.goto(`/${slug}/`); await expect(page.locator('#blotter')).toHaveCount(0);
    await page.goto('/blotter'); await expect(page.getByText('No blotter messages.', { exact: true })).toBeVisible();
    const entries = [];
    for (let index = 0; index < 27; index++) entries.push(await publish(`Announcement ${index}`));
    const hostile = await publish('<img src=x onerror="window.blotterInjected=1">\n<script>window.blotterInjected=1</script>');
    await page.goto(`/${slug}/`);
    await expect(page.locator('#blotter-msgs tr')).toHaveCount(3);
    await expect(page.locator('#blotter-msgs tr').first().locator('.blotterMessage')).toHaveText(hostile.content);
    await expect(page.locator('#blotter img, #blotter script')).toHaveCount(0);
    await page.getByRole('link', { name: 'Hide', exact: true }).click();
    await expect(page.locator('#blotter-all')).toBeHidden(); await page.reload();
    await expect(page.getByRole('link', { name: 'Show Blotter' })).toBeVisible();
    await page.getByRole('link', { name: 'Show Blotter' }).click();
    expect(await page.evaluate(() => localStorage.getItem('4chan-blotter'))).toBeNull();
    await page.getByRole('link', { name: 'Hide', exact: true }).click();
    const latest = await publish('New message after dismissal'); await page.reload();
    await expect(page.locator('#blotter-msgs')).toBeVisible();
    await expect(page.locator('#blotter-msgs tr').first().locator('.blotterMessage')).toHaveText(latest.content);
    const [listing] = await Promise.all([page.waitForEvent('popup'),
      Promise.resolve().then(() => page.getByRole('link', { name: 'Show All' }).click())]);
    await expect(listing).toHaveURL(/\/blotter$/);
    expect(await listing.evaluate(() => window.opener)).toBeNull();
    await expect(listing.locator('#entries tbody tr')).toHaveCount(25);
    await expect(listing.locator('#entries tbody tr').first()).toHaveAttribute('id', `msg-${latest.id}`);
    await expect(listing.locator(`#msg-${hostile.id} .blotterMessage`)).toHaveText(hostile.content);
    const firstIds = await listing.locator('#entries tbody tr').evaluateAll(rows => rows.map(row => row.id));
    await expect(listing.getByRole('link', { name: 'Next', exact: true })).toHaveAttribute('href', `/blotter?offset=${firstIds.at(-1).slice(4)}`);
    await listing.getByRole('link', { name: 'Next', exact: true }).click();
    await expect(listing.locator('#entries tbody tr')).toHaveCount(4);
    const nextIds = await listing.locator('#entries tbody tr').evaluateAll(rows => rows.map(row => row.id));
    expect(nextIds.some(id => firstIds.includes(id))).toBe(false);
    await expect(listing.getByRole('link', { name: 'Next', exact: true })).toHaveCount(0);
    await listing.close();
    await page.goto(`/${disabled}/`); await expect(page.locator('#blotter')).toHaveCount(0);
    await page.setViewportSize({ width: 375, height: 812 }); await page.goto(`/${slug}/`);
    await expect(page.locator('#blotter')).toBeHidden(); await page.goto('/blotter');
    await expect(page.locator(`#msg-${hostile.id}`)).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect(await page.evaluate(() => window.blotterInjected)).toBeUndefined();
  });
});
