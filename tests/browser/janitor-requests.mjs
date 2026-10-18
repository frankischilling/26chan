import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { chromium, expect } from '@playwright/test';

for (const name of Object.keys(process.env)) {
  assert(!name.endsWith('DATABASE_URL') && !['GH_CONFIG_DIR', 'POSTER_ID_KEY', 'PUBLIC_INTAKE_TOKEN'].includes(name),
    'staff browser must not inherit database or service credentials');
}
const origin = process.env.STAFF_REQUEST_ORIGIN;
assert.equal(origin, 'http://localhost:3001');
const input = JSON.parse(process.env.STAFF_REQUEST_TARGETS);
assert.match(input.board, /^jq[0-9a-f]+$/);
assert.equal(input.posts.length, 4);
assert(input.posts.every(id => Number.isSafeInteger(id) && id > 0));
assert(Number.isSafeInteger(input.template) && input.template > 0);
const cookies = process.env.STAFF_REQUEST_COOKIE.split(';').map(field => {
  const [name, value] = field.trim().split('=');
  assert(['staff', 'staff-csrf'].includes(name));
  assert.match(value, /^[A-Za-z0-9_-]{43}$/);
  return { name, value, url: origin, httpOnly: true, sameSite: 'Strict' };
});
assert.equal(cookies.length, 2);
const output = path.resolve('test-results/janitor-requests');
await mkdir(output, { recursive: true });
const browser = await chromium.launch();
try {
  let index = 0;
  for (const javaScriptEnabled of [true, false]) {
    for (const [size, viewport] of [['desktop', { width: 1280, height: 900 }], ['mobile', { width: 390, height: 844 }]]) {
      const context = await browser.newContext({ javaScriptEnabled, viewport });
      try {
        await context.addCookies(cookies);
        await context.route('**/*', route => new URL(route.request().url()).origin === origin ? route.continue() : route.abort());
        const page = await context.newPage();
        const prefix = `${size}-${javaScriptEnabled ? 'js' : 'no-js'}`;
        const response = await page.goto(`${origin}/ban-request?board=${input.board}&target=${input.posts[index]}`, { waitUntil: 'networkidle' });
        assert.equal(response.status(), 200);
        await expect(page.getByRole('combobox', { name: 'Template', exact: true })).toBeVisible();
        await page.getByRole('combobox', { name: 'Template', exact: true }).selectOption(String(input.template));
        await page.screenshot({ fullPage: true, path: path.join(output, `${prefix}-form.png`) });
        await page.getByRole('button', { name: 'Submit request', exact: true }).click();
        await expect(page.locator('#content')).toContainText('is waiting for moderator review');
        const match = (await page.locator('#content').innerText()).match(/Request ([0-9]+) is waiting/);
        assert(match);
        const id = match[1];
        await page.goto(`${origin}/ban-requests?board=${input.board}`, { waitUntil: 'networkidle' });
        const row = page.locator(`#ban-request-${id}`);
        await expect(row).toContainText('Owned HTTP request body');
        await expect(row.getByRole('button', { name: /^Deny/ })).toBeVisible();
        await page.screenshot({ fullPage: true, path: path.join(output, `${prefix}-queue.png`) });
        await row.getByRole('button', { name: /^Deny/ }).click();
        await expect(page.locator('#content')).toContainText('Request denied. 1 pending request(s) cleared.');
        await page.screenshot({ fullPage: true, path: path.join(output, `${prefix}-review.png`) });
        await page.goBack({ waitUntil: 'networkidle' });
        await expect(page.locator(`#ban-request-${id}`)).toHaveCount(0);
        await page.goto(`${origin}/ban-requests/statistics`, { waitUntil: 'networkidle' });
        await expect(page.locator('#content')).toContainText(`My requests: 1 approved, ${index + 2} denied, 0 pending.`);
        console.log(`PASS ${prefix}: real creation, captured queue, denial, Back navigation and counted statistics`);
        index += 1;
      } finally {
        await context.close();
      }
    }
  }
} finally {
  await browser.close();
}
