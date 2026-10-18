import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from 'playwright';

// This driver is called only by the owned Rust/DB fixture. No public service or
// existing browser profile is used, and no credentials are printed or saved.
const origin = process.env.STAFF_MESSAGE_ORIGIN;
assert.equal(origin, 'http://localhost:3001');
const cookies = process.env.STAFF_MESSAGE_COOKIE.split(';').map(field => {
  const [name, value] = field.trim().split('=');
  assert(['staff', 'staff-csrf'].includes(name));
  assert.match(value, /^[A-Za-z0-9_-]{43}$/);
  return { name, value, url: origin, httpOnly: true, sameSite: 'Strict' };
});
assert.equal(cookies.length, 2);
const root = path.resolve(import.meta.dirname, '../..');
const references = JSON.parse(await readFile(path.join(root, 'fixtures/staff-messages-pages.json'), 'utf8'));
const sourceCss = await readFile(path.join(root, 'fixtures/staff-reportqueue-source.css'));
const output = path.join(root, 'test-results/staff-messages');
await mkdir(output, { recursive: true });
const browser = await chromium.launch({
  executablePath: process.env.STAFF_MESSAGE_BROWSER_EXECUTABLE || undefined,
});
try {
  for (const [name, viewport] of [['desktop', { width: 1280, height: 900 }], ['mobile', { width: 390, height: 844 }]]) {
    for (const javaScriptEnabled of [true, false]) {
      const context = await browser.newContext({ viewport, javaScriptEnabled, deviceScaleFactor: 1 });
      try {
        await context.addCookies([...cookies, { name: 'staff-messages', value: '0', url: origin }]);
        await context.route('**/*', async route => {
          const url = new URL(route.request().url());
          if (url.origin !== origin) return route.abort();
          if (url.pathname === '/reference-messages') return route.fulfill({ contentType: 'text/html', body: references[url.searchParams.get('state') || 'unread'] });
          if (url.pathname === '/css/reportqueue.css') return route.fulfill({ contentType: 'text/css', body: sourceCss });
          if (url.pathname === '/image/favicon-team.ico') return route.fulfill({ contentType: 'image/x-icon', body: await readFile(path.join(root, 'apps/staff/static/favicon-team.ico')) });
          return route.continue();
        });
        const page = await context.newPage();
        const prefix = `${name}-${javaScriptEnabled ? 'js' : 'no-js'}`;
        const compare = async (state, label = state) => {
          await page.evaluate(() => document.fonts.ready);
          const native = await page.screenshot({ fullPage: true, path: path.join(output, `${prefix}-${label}-native.png`) });
          await page.goto(`${origin}/reference-messages?state=${state}`, { waitUntil: 'networkidle' });
          await page.evaluate(() => document.fonts.ready);
          const expected = await page.screenshot({ fullPage: true, path: path.join(output, `${prefix}-${label}-reference.png`) });
          assert(native.equals(expected), `${prefix}-${label}: native staff messages differ from the independent source page`);
          console.log(`PASS ${prefix}-${label}: complete source staff-message page matches`);
        };
        for (const [state, seen, offset, rows] of [
          ['unread', '0', 0, 3],
          ['mixed', '1700000001', 0, 3],
          ['read', '2000000000', 0, 3],
          ['empty', '0', 25, 0],
        ]) {
          await context.addCookies([{ name: 'staff-messages', value: seen, url: origin }]);
          const before = await context.request.get(`${origin}/messages/count`);
          assert.equal(before.status(), 200);
          assert.equal(before.headers()['set-cookie'], undefined);
          assert.deepEqual(await before.json(), { unread: state === 'read' ? 0 : state === 'mixed' ? 1 : 3 });
          const response = await page.goto(`${origin}/messages?offset=${offset}`, { waitUntil: 'networkidle' });
          assert.equal(response.status(), 200);
          assert.equal(await page.locator('tbody tr').count(), rows);
          assert.equal(await page.locator('tbody').innerText().then(text => text.includes('Owned hidden message')), false);
          assert.equal(await page.locator('#menu a').getAttribute('href'), '/reports');
          const after = await context.request.get(`${origin}/messages/count`);
          assert.deepEqual(await after.json(), { unread: 0 });
          await compare(state);
        }
        // Reopening a page consumes the updated marker, including with scripts
        // disabled. Navigate back through the actual Previous link as well.
        await page.goto(`${origin}/messages?offset=25`, { waitUntil: 'networkidle' });
        await page.getByRole('link', { name: 'Previous' }).click();
        await page.waitForURL(`${origin}/messages?offset=0`);
        await page.waitForLoadState('networkidle');
        assert.equal(await page.locator('tbody tr.item-read').count(), 1);
        await compare('read', 'previous');
        await page.goto(`${origin}/messages`, { waitUntil: 'networkidle' });
        await page.reload({ waitUntil: 'networkidle' });
        await compare('read', 'reload');
      } finally {
        await context.close();
      }
    }
  }
} finally {
  await browser.close();
}
