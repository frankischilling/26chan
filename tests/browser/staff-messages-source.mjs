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
const reference = await readFile(path.join(root, 'fixtures/staff-messages-reference.html'));
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
          if (url.pathname === '/reference-messages') return route.fulfill({ contentType: 'text/html', body: reference });
          if (url.pathname === '/css/reportqueue.css') return route.fulfill({ contentType: 'text/css', body: sourceCss });
          if (url.pathname === '/image/favicon-team.ico') return route.fulfill({ contentType: 'image/x-icon', body: await readFile(path.join(root, 'apps/staff/static/favicon-team.ico')) });
          return route.continue();
        });
        const page = await context.newPage();
        const response = await page.goto(`${origin}/messages`, { waitUntil: 'networkidle' });
        assert.equal(response.status(), 200);
        assert.equal(await page.locator('tbody tr').count(), 3);
        assert.equal(await page.locator('tbody').innerText().then(text => text.includes('Owned hidden message')), false);
        const prefix = `${name}-${javaScriptEnabled ? 'js' : 'no-js'}`;
        const native = await page.screenshot({ fullPage: true, path: path.join(output, `${prefix}-native.png`) });
        await page.goto(`${origin}/reference-messages`, { waitUntil: 'networkidle' });
        const expected = await page.screenshot({ fullPage: true, path: path.join(output, `${prefix}-reference.png`) });
        assert(native.equals(expected), `${prefix}: native staff messages differ from the independent source page`);
        console.log(`PASS ${prefix}: complete source staff-message page matches`);
      } finally {
        await context.close();
      }
    }
  }
} finally {
  await browser.close();
}
