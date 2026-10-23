import assert from 'node:assert/strict';
import { chromium, expect } from '@playwright/test';
const origin = process.argv[2];
assert.match(origin, /^http:\/\/127\.0\.0\.1:\d+$/);
const browser = await chromium.launch();
try {
  let iteration = 0;
  for (const javaScriptEnabled of [false, true]) {
    for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
      const context = await browser.newContext({ javaScriptEnabled, viewport });
      const page = await context.newPage();
      page.setDefaultTimeout(12000);
      await page.goto(`${origin}/event-market.html`);
      await expect(page.locator('.eventTable tbody tr')).toHaveCount(18);
      await page.getByRole('button', { name: 'Create account', exact: true }).click();
      await expect(page.locator('.eventBalance')).toContainText(`${1000 - iteration * 100} points`);
      if (iteration > 0) await page.waitForTimeout(11000);
      const row = page.locator('.eventTable tbody tr').first();
      await row.getByRole('button', { name: 'Buy', exact: true }).click();
      await expect(page.locator('.eventBalance')).toHaveText(`Balance: ${900 - iteration * 100} points`);
      await page.reload();
      await expect(page.locator('.eventBalance')).toHaveText(`Balance: ${900 - iteration * 100} points`);
      await page.goto(`${origin}/event-emotes.html`);
      await page.getByRole('button', { name: 'Start collecting', exact: true }).click();
      await expect(page.locator('.eventRoster > li')).toHaveCount(166);
      const choice = page.locator('.eventRoster > li').filter({ has: page.locator('.eventGlyph') }).filter({ has: page.locator('button:not([disabled])') }).first();
      const name = await choice.locator('strong').textContent();
      await choice.getByRole('button', { name: 'Buy', exact: true }).click();
      await expect(page.locator('.eventObtained')).toHaveText(`Obtained: ${name}`);
      const balance = await page.locator('.eventBalance').textContent();
      await page.reload();
      await expect(page.locator('.eventBalance')).toHaveText(balance);
      await page.getByRole('button', { name: 'Roll 3 (30 points)', exact: true }).click();
      await expect(page.locator('.eventObtained')).toBeVisible();
      const obtained = (await page.locator('.eventObtained').textContent()).replace('Obtained: ', '').split(', ');
      assert.equal(obtained.length, 3);
      const cookies = await context.cookies();
      for (const cookie of cookies.filter(c => c.name.startsWith('board-event-'))) {
        assert.equal(cookie.httpOnly, true);
        assert.equal(cookie.sameSite, 'Strict');
        assert.ok(!(await page.content()).includes(cookie.value));
      }
      assert.equal(await page.locator('script, iframe').count(), 0);
      await context.close();
      iteration++;
    }
  }
} finally { await browser.close(); }
console.log('Native market/emote forms passed with and without scripts, desktop and mobile.');
