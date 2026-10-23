import assert from 'node:assert/strict';
import readline from 'node:readline';
import { mkdir } from 'node:fs/promises';
import { chromium, expect } from '@playwright/test';
const input = readline.createInterface({ input: process.stdin });
const lines = input[Symbol.asyncIterator]();
async function read() { const next = await lines.next(); assert.equal(next.done, false); return JSON.parse(next.value); }
async function rpc(request) { process.stdout.write(`${JSON.stringify(request)}\n`); return read(); }
const { origin, board, posts } = await read();
assert.match(origin, /^http:\/\/127\.0\.0\.1:\d+$/);
assert.match(board, /^[a-z0-9]{1,10}$/);
assert.equal(posts.length, 4);
await mkdir('test-results/events-score', { recursive: true });
const browser = await chromium.launch();
try {
  let index = 0;
  for (const javaScriptEnabled of [false, true]) {
    for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
      assert.equal((await rpc({ step: 'prepare' })).ready, true);
      const post = posts[index++];
      const context = await browser.newContext({ javaScriptEnabled, viewport });
      const page = await context.newPage();
      page.setDefaultTimeout(12000);
      const path = `${origin}/event-like/${board}/${post}`;
      const response = await page.goto(path);
      assert.equal(response.status(), 200);
      await expect(page.getByRole('heading', { name: `Like No.${post}` })).toBeVisible();
      await expect(page.locator('img[alt="CAPTCHA characters"]')).toBeVisible();
      const token = await page.locator('input[name="challenge"]').getAttribute('value');
      const { answer } = await rpc({ step: 'answer', token, post });
      await page.getByLabel('Verification', { exact: true }).fill(answer);
      await page.screenshot({ path: `test-results/events-score/${javaScriptEnabled ? 'scripts' : 'native'}-${viewport.width}-form.png`, fullPage: true });
      await page.getByRole('button', { name: 'Like this post', exact: true }).click();
      await expect(page).toHaveURL(`${path}?view=1`);
      await expect(page.locator('main')).toContainText('This post has 1 like.');
      await page.reload();
      await expect(page.locator('main')).toContainText('This post has 1 like.');
      assert.equal(await page.locator('script, iframe, input[name="challenge"]').count(), 0);
      for (const cookie of (await context.cookies()).filter(c => ['board-anon', 'board-captcha'].includes(c.name))) {
        assert.equal(cookie.httpOnly, true); assert.equal(cookie.sameSite, 'Strict');
        assert.ok(!(await page.content()).includes(cookie.value));
      }
      await context.close();
    }
  }
  process.stdout.write(`${JSON.stringify({ step: 'done' })}\n`);
} finally { await browser.close(); input.close(); }
