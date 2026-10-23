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
try {
  for (const javaScriptEnabled of [false, true]) {
    for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
      assert.equal((await rpc({ step: 'prepare' })).ready, true);
      const context = await browser.newContext({ javaScriptEnabled, viewport });
      const page = await context.newPage();
      page.setDefaultTimeout(12000);
      const response = await page.goto(`${origin}/feedback`);
      assert.equal(response.status(), 200);
      const anonymous = (await context.cookies()).find(cookie => cookie.name === 'board-anon');
      assert.ok(anonymous);
      assert.equal((await rpc({ step: 'session', token: anonymous.value })).recorded, true);
      await expect(page.getByRole('heading', { name: 'Submit Feedback', exact: true })).toBeVisible();
      await expect(page.locator('form')).toHaveAttribute('enctype', 'multipart/form-data');
      await expect(page.locator('#form-cat option')).toHaveText(['<Owned>']);
      await expect(page.locator('#form-subject')).toHaveAttribute('maxlength', '150');
      await expect(page.locator('#form-message')).toHaveAttribute('maxlength', '2000');
      await page.getByLabel('Subject:', { exact: true }).fill(`Owned browser ${javaScriptEnabled}-${viewport.width}`);
      await page.getByLabel('Message:', { exact: true }).fill('<script>literal browser feedback</script>\nSecond line');
      const token = await page.locator('input[name="challenge"]').getAttribute('value');
      const { answer } = await rpc({ step: 'answer', token });
      await page.getByLabel('Verification:', { exact: true }).fill(answer);
      await page.getByRole('button', { name: 'Submit', exact: true }).click();
      await expect(page).toHaveURL(`${origin}/feedback?sent=1`);
      await expect(page.locator('.feedback.success')).toHaveText('Your feedback has been submitted.');
      for (let refresh = 0; refresh < 2; refresh++) {
        await page.reload();
        await expect(page.locator('.feedback.success')).toBeVisible();
        assert.equal(await page.locator('form, script, input[name="challenge"]').count(), 0);
      }
      for (const cookie of (await context.cookies()).filter(c => ['board-anon', 'board-captcha'].includes(c.name))) {
        assert.equal(cookie.httpOnly, true); assert.equal(cookie.sameSite, 'Strict');
        assert.ok(!(await page.content()).includes(cookie.value));
      }
      await context.close();
    }
  }
  process.stdout.write(`${JSON.stringify({ step: 'done' })}\n`);
} finally { await browser.close(); input.close(); }
