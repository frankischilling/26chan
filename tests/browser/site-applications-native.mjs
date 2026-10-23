import assert from 'node:assert/strict';
import readline from 'node:readline';
import { chromium, expect } from '@playwright/test';
const input = readline.createInterface({ input: process.stdin });
const lines = input[Symbol.asyncIterator]();
async function read() { const next = await lines.next(); assert.equal(next.done, false); return JSON.parse(next.value); }
async function rpc(value) { process.stdout.write(`${JSON.stringify(value)}\n`); return read(); }
const { origin } = await read();
assert.match(origin, /^http:\/\/127\.0\.0\.1:\d+$/);
const browser = await chromium.launch();
async function prepare() { assert.equal((await rpc({ step: 'prepare' })).ready, true); }
async function solve(page) {
  const token = await page.locator('input[name="challenge"]').getAttribute('value');
  const { answer } = await rpc({ step: 'answer', token });
  await page.locator('input[name="answer"]').fill(answer);
}
try {
  for (const javaScriptEnabled of [false, true]) {
    const mobile = await browser.newContext({ javaScriptEnabled, viewport: { width: 390, height: 844 }, userAgent: 'Owned Android Mobile' });
    const mobilePage = await mobile.newPage();
    await mobilePage.goto(`${origin}/janitorapp`);
    await expect(mobilePage.locator('.closed-msg')).toContainText('Please use a desktop browser');
    assert.equal(await mobilePage.locator('form').count(), 0);
    assert.equal((await mobile.cookies()).length, 0);
    await mobile.close();
    await prepare();
    const context = await browser.newContext({ javaScriptEnabled, viewport: { width: 1280, height: 900 }, timezoneId: 'America/New_York' });
    const page = await context.newPage(); page.setDefaultTimeout(12000);
    const response = await page.goto(`${origin}/janitorapp`);
    assert.equal(response.status(), 200); assert.equal(response.headers()['referrer-policy'], 'strict-origin');
    const anonymous = (await context.cookies()).find(c => c.name === 'board-anon'); assert.ok(anonymous);
    assert.equal((await rpc({ step: 'session', token: anonymous.value })).recorded, true);
    await expect(page.locator('#application-form')).toBeVisible();
    await expect(page.locator('#js-tz option')).toHaveCount(26);
    const email = `owned-browser-${javaScriptEnabled}@example.org`;
    for (const [name, value] of Object.entries({ firstname: 'Owned <browser>', handle: '<b>literal handle</b>', email, age: '25', hours: '4', times: '16:00-20:00', q1: 'One\nSecond line', q2: 'Two', q3: 'Three', q4: 'Four' })) {
      await page.locator(`[name="${name}"]`).fill(value);
    }
    await page.locator('[name="board1"]').selectOption('g');
    await page.locator('#js-tz').selectOption('-7');
    await solve(page); await page.getByRole('button', { name: 'Submit', exact: true }).click();
    await expect(page.locator('#success')).toHaveText('Application submitted.');
    const editPath = await page.locator('.success-sub a').getAttribute('href'); assert.match(editPath, /^\/janitorapp\?id=app1\.[A-Za-z0-9_-]{43}$/);
    await prepare(); await page.goto(`${origin}${editPath}`);
    assert.equal(await page.locator('[name="firstname"]').count(), 0);
    await page.locator('[name="auth_email"]').fill(email);
    await solve(page); await prepare(); // Auth is consumed before the next edit challenge is issued.
    await page.getByRole('button', { name: 'Submit', exact: true }).click();
    await expect(page.locator('[name="firstname"]')).toHaveValue('Owned <browser>');
    await expect(page.locator('#js-tz')).toHaveAttribute('data-edit', '');
    await expect(page.locator('#js-tz')).toHaveValue('-7');
    await prepare(); await page.getByRole('button', { name: 'Refresh verification', exact: true }).click();
    await expect(page.locator('[name="firstname"]')).toHaveValue('Owned <browser>');
    await page.locator('[name="q1"]').fill('Edited literal <answer>');
    await solve(page); await page.getByRole('button', { name: 'Submit', exact: true }).click();
    await expect(page.locator('#success')).toHaveText('Application submitted.');
    await expect(page.locator('.success-sub a')).toHaveAttribute('href', editPath);
    await context.close();
  }
  process.stdout.write(`${JSON.stringify({ step: 'done' })}\n`);
} finally { await browser.close(); input.close(); }
