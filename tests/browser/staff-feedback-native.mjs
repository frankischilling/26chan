import assert from 'node:assert/strict';
import readline from 'node:readline';
import { chromium, expect } from '@playwright/test';
const input = readline.createInterface({ input: process.stdin });
const lines = input[Symbol.asyncIterator]();
const first = await lines.next();
assert.equal(first.done, false);
const { origin, cookie, profile, pairs } = JSON.parse(first.value);
input.close();
assert.equal(origin, 'http://localhost:3001');
assert.ok(Number.isSafeInteger(profile) && profile > 0);
assert.equal(pairs.length, 4);
const cookies = cookie.split('; ').map(part => {
  const index = part.indexOf('=');
  const name = part.slice(0, index), value = part.slice(index + 1);
  assert.ok(['staff', 'staff-csrf'].includes(name));
  assert.match(value, /^[A-Za-z0-9_-]{43}$/);
  return { name, value, url: origin, httpOnly: true, sameSite: 'Strict' };
});
const browser = await chromium.launch();
try {
  let index = 0;
  for (const javaScriptEnabled of [false, true]) {
    for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
      const [pending, draft] = pairs[index++];
      assert.ok([pending, draft].every(id => Number.isSafeInteger(id) && id > 0));
      const context = await browser.newContext({ javaScriptEnabled, viewport });
      await context.addCookies(cookies);
      const page = await context.newPage();
      page.setDefaultTimeout(12000);
      const response = await page.goto(`${origin}/feedback?profile=${profile}`);
      assert.equal(response.status(), 200);
      await expect(page.getByRole('heading', { name: '4chan Feedback', exact: true })).toBeVisible();
      const form = id => page.locator('form').filter({ has: page.locator(`input[name="id"][value="${id}"]`) });
      await form(pending).getByRole('button', { name: 'Dismiss', exact: true }).click();
      await expect(page).toHaveURL(`${origin}/feedback?action=review&profile=${profile}`);
      await expect(form(pending)).toHaveCount(0);
      await page.reload();
      await expect(form(pending)).toHaveCount(0);
      await page.goto(`${origin}/feedback?profile=${profile}&answered=3`);
      await form(draft).getByRole('button', { name: 'Approve', exact: true }).click();
      await expect(page).toHaveURL(`${origin}/feedback?action=review&profile=${profile}&answered=3`);
      await expect(form(draft)).toHaveCount(0);
      await page.goto(`${origin}/feedback?profile=${profile}&answered=1`);
      const link = page.getByRole('link', { name: String(draft), exact: true });
      await expect(link).toBeVisible();
      await link.click();
      await expect(page).toHaveURL(`${origin}/feedback?action=read&id=${draft}`);
      await expect(page.locator('.uid')).toHaveText('192.0.2.225');
      assert.equal(await page.locator('form, textarea, script, iframe').count(), 0);
      await page.reload();
      await expect(page.locator('.uid')).toHaveText('192.0.2.225');
      await context.close();
    }
  }
} finally { await browser.close(); }
