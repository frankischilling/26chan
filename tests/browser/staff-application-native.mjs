import assert from 'node:assert/strict';
import readline from 'node:readline';
import { chromium, expect } from '@playwright/test';
const input = readline.createInterface({ input: process.stdin });
const first = await input[Symbol.asyncIterator]().next();
assert.equal(first.done, false);
const { origin, cookie, campaign, pairs } = JSON.parse(first.value);
input.close();
assert.equal(origin, 'http://localhost:3001');
assert.ok(Number.isSafeInteger(campaign) && campaign > 0);
assert.equal(pairs.length, 4);
const cookies = cookie.split('; ').map(part => {
  const index = part.indexOf('='), name = part.slice(0, index), value = part.slice(index + 1);
  assert.ok(['staff', 'staff-csrf'].includes(name));
  assert.match(value, /^[A-Za-z0-9_-]{43}$/);
  return { name, value, url: origin, httpOnly: true, sameSite: 'Strict' };
});
const browser = await chromium.launch();
try {
  let index = 0;
  for (const javaScriptEnabled of [false, true]) {
    for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
      const [selected, declined] = pairs[index++];
      assert.ok([selected, declined].every(id => Number.isSafeInteger(id) && id > 0));
      const context = await browser.newContext({ javaScriptEnabled, viewport });
      await context.addCookies(cookies);
      const page = await context.newPage();
      page.setDefaultTimeout(12000);
      const response = await page.goto(`${origin}/applications?campaign=${campaign}`);
      assert.equal(response.status(), 200);
      assert.equal(response.headers()['cache-control'], 'private, no-store');
      await expect(page.getByRole('heading', { name: 'Application Review', exact: true })).toBeVisible();
      await page.getByRole('link', { name: String(selected), exact: true }).click();
      await expect(page.getByText('Owned browser answer', { exact: true })).toBeVisible();
      await expect(page.locator('body')).not.toContainText('Owned browser name');
      await expect(page.locator('body')).not.toContainText('@example.invalid');
      await page.getByRole('button', { name: 'Select applicant', exact: true }).click();
      await expect(page).toHaveURL(`${origin}/applications?id=${selected}`);
      await expect(page.getByRole('button', { name: 'Select applicant', exact: true })).toHaveCount(0);
      await page.reload();
      await expect(page.locator('body')).not.toContainText('@example.invalid');
      await page.getByRole('link', { name: 'View selected applicant contact', exact: true }).click();
      await expect(page.locator('body')).toContainText('Owned browser name');
      await expect(page.locator('body')).toContainText('@example.invalid');
      await page.goto(`${origin}/applications?id=${declined}`);
      await page.getByRole('button', { name: 'Decline', exact: true }).click();
      await expect(page).toHaveURL(`${origin}/applications?id=${declined}`);
      await expect(page.locator('body')).not.toContainText('@example.invalid');
      await expect(page.getByRole('link', { name: 'View selected applicant contact', exact: true })).toHaveCount(0);
      const denied = await page.goto(`${origin}/applications?id=${declined}&identity=true`);
      assert.equal(denied.status(), 404);
      assert.equal(await page.locator('script,iframe').count(), 0);
      await context.close();
    }
  }
} finally { await browser.close(); }
