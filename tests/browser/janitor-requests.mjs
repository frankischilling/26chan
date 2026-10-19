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
const browser = await chromium.launch(process.env.CHROMIUM_EXECUTABLE_PATH ? { executablePath: process.env.CHROMIUM_EXECUTABLE_PATH } : {});
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
        await expect(row.locator('.user-bans-cnt')).toContainText('Past 12 months ban history');
        await expect(row.locator('.br-tpl-reason')).toHaveText('Owned <script>reason</script>');
        await expect(row.getByRole('button', { name: /^Deny/ })).toBeVisible();
        const thumbnail=row.locator('.post-thumb');
        await expect(thumbnail).toBeVisible();
        await expect.poll(()=>thumbnail.evaluate(image=>image.complete && image.naturalWidth)).toBe(200);
        await expect(thumbnail).toHaveAttribute('width','130');
        await expect(thumbnail).toHaveAttribute('height','65');
        if(javaScriptEnabled) {
          await thumbnail.hover(); await expect(page.locator('#image-hover')).toBeVisible();
          await page.mouse.move(0,0); await expect(page.locator('#image-hover')).toHaveCount(0);
          await row.locator('.post-no').click(); await page.keyboard.press('i');
          await expect(page.locator('#image-hover')).toBeVisible();
          await page.keyboard.press('i'); await expect(page.locator('#image-hover')).toHaveCount(0);
        }
        await page.screenshot({ fullPage: true, path: path.join(output, `${prefix}-queue.png`) });
        if (index === 0) {
          // Reproduce the incompatible source policy without weakening the POST guard.
          await page.evaluate(() => document.querySelector('meta[name="referrer"]').content = 'no-referrer');
          const [refused] = await Promise.all([
            page.waitForResponse(response => response.url() === `${origin}/ban-requests/review` && response.request().method() === 'POST'),
            row.locator('form').first().evaluate(form => {
              const decision = document.createElement('input');
              decision.type = 'hidden'; decision.name = 'decision'; decision.value = 'deny';
              form.append(decision);
              HTMLFormElement.prototype.submit.call(form);
            }),
          ]);
          assert.equal(await refused.request().headerValue('origin'), 'null');
          assert.equal(refused.status(), 403, 'opaque-origin form must remain forbidden');
          await page.waitForURL(`${origin}/ban-requests/review`, { waitUntil: 'domcontentloaded' });
          await page.goto(`${origin}/ban-requests?board=${input.board}`, { waitUntil: 'networkidle' });
          await expect(row).toContainText('Owned HTTP request body');
        }
        if (javaScriptEnabled) {
          await row.locator('.request-amend summary').click();
          await row.locator('[name="public_reason"]').fill('Old override');
          await row.getByLabel('Find template').fill('Owned request');
          await expect(row.locator('[name="public_reason"]')).toHaveValue('Owned <script>reason</script>');
          await expect(row.locator('[name="ban_length"][type="number"]')).toHaveValue('1');
          await row.locator('select[name="amend_tpl"]').selectOption(String(input.template + 1));
          await expect(row.locator('[name="ban_length"][type="number"]')).toHaveValue('0');
          await row.locator('.request-amend summary').click();
          await page.getByText('Settings', {exact:true}).click();
          await page.locator('[data-queue-key="w"]').fill('v');
          await page.locator('#queue-custom-menu').fill(input.board);
          await page.locator('#queue-hide-thumbnails').check();
          await page.locator('#queue-save-preferences').click();
          await page.reload();
          await page.getByText('Settings', {exact:true}).click();
          await expect(page.locator('[data-queue-key="w"]')).toHaveValue('v');
          await expect(page.locator('#queue-custom-menu')).toHaveValue(input.board);
          await expect(row.locator('.post-thumb')).toHaveCSS('opacity','0');
          await page.locator('#queue-hide-thumbnails').uncheck();
          await page.locator('#queue-save-preferences').click();
          await expect(row.locator('.post-thumb')).toHaveCSS('opacity','1');
          await page.getByText('Settings', {exact:true}).click();
          await page.getByLabel('Filter loaded requests').fill('no-such-owned-request');
          await page.getByLabel('Filter loaded requests').press('Enter');
          await expect(row).toBeHidden();
          await page.getByRole('button', { name: 'Clear filter' }).click();
          await expect(row).toBeVisible();
          await row.getByRole('button', { name: /^Deny/ }).click();
          await page.getByRole('button', { name: 'Undo pending review' }).click();
          await expect(row).toBeVisible();
          await expect(page.locator('#queue-status')).toContainText('Nothing was submitted');
          await page.keyboard.press('v');
          await expect(page.locator('#queue-undo')).toBeVisible();
          await page.locator('#queue-undo').click();
          await expect(row).toBeVisible();
        }
        const [reviewResponse] = await Promise.all([
          page.waitForResponse(response => response.url() === `${origin}/ban-requests/review` && response.request().method() === 'POST'),
          row.getByRole('button', { name: /^Deny/ }).click(),
        ]);
        assert.equal(await reviewResponse.request().headerValue('origin'), origin,
          'native form must retain its same-origin proof');
        assert.equal(reviewResponse.status(), 200, 'authorized review form must succeed');
        const gone=await context.request.get(`${origin}/ban-requests/media/${id}/file`);
        assert.equal(gone.status(),404,'cleared request must lose private media access');
        await expect(page.locator(javaScriptEnabled ? '#queue-status' : '#content')).toContainText('Request denied. 1 pending request(s) cleared.');
        await page.screenshot({ fullPage: true, path: path.join(output, `${prefix}-review.png`) });
        if (javaScriptEnabled) {
          assert.equal(new URL(page.url()).pathname, '/ban-requests', 'enhanced review stays in the queue');
          await expect(row).toHaveCount(0);
          await page.goto(`${origin}/ban-requests/statistics`, { waitUntil: 'networkidle' });
        }
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
