import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';
const [origin, board] = process.argv.slice(2);
assert.match(board, /^[a-z0-9]{10}$/);
const browser = await chromium.launch({ headless: true });
const errors = [];
try {
  // The actual socket peer is loopback, so spoofed GeoIP headers still yield XX.
  const plain = await browser.newContext({ javaScriptEnabled: false, extraHTTPHeaders: {
    'CF-IPCountry': 'US', 'X-Forwarded-For': '81.2.69.142',
  } });
  const page = await plain.newPage(); page.setDefaultTimeout(10000);
  await page.goto(`${origin}/${board}/`);
  assert.deepEqual(await page.locator('select[name=flag] option').evaluateAll(options => options.map(o => [o.value, o.textContent])),
    [['0', 'Geographic Location'], ['AC', 'Anarcho-Capitalist'], ['UN', 'United Nations']]);
  await page.locator('#sub').fill('Owned flags browser'); await page.locator('#com').fill('Owned geographic post');
  await page.locator('#password').fill('owned-flags-browser-password');
  await Promise.all([page.waitForURL(/\/thread\/[0-9]+#p[0-9]+$/), page.locator('#sub').locator('..').locator('button').click()]);
  const thread = new URL(page.url()).pathname.split('/').pop();
  const country = page.locator('.postInfo .flag-xx'); assert.equal(await country.count(), 1);
  assert.equal(await country.getAttribute('title'), 'Unknown');
  assert.equal(await country.evaluate(node => getComputedStyle(node).width), '16px');
  assert.equal(await country.evaluate(node => getComputedStyle(node).height), '11px');
  const unknownPosition = await country.evaluate(node => getComputedStyle(node).backgroundPosition);
  assert.notEqual(unknownPosition, '0% 0%');
  await plain.close();
  const active = await browser.newContext();
  const live = await active.newPage(); live.setDefaultTimeout(10000);
  live.on('pageerror', error => errors.push(error.message));
  await live.goto(`${origin}/${board}/thread/${thread}`);
  await live.locator('.postNum').first().click();
  await live.locator('#quickReply #qrFlag').selectOption('UN');
  await live.locator('#qrCom').fill('Owned selected board flag');
  await live.locator('#quickReply input[name=pwd]').fill('owned-flags-browser-password');
  const posted = live.waitForResponse(response => new URL(response.url()).pathname === `/${board}/imgboard.php`)
    .then(async response => ({ status: response.status(), receipt: await response.json() }));
  await live.locator('#quickReply input[type=submit]').click();
  const result = await posted; assert.equal(result.status, 200);
  const receipt = result.receipt; assert.ok(receipt.pid);
  const flag = live.locator(`#pi${receipt.pid} .bfl-un`); await flag.waitFor();
  assert.equal(await flag.getAttribute('title'), 'United Nations');
  assert.equal(await flag.evaluate(node => getComputedStyle(node).width), '16px');
  const json = await (await active.request.get(`${origin}/${board}/thread/${thread}.json`)).json();
  const saved = json.posts.find(p => p.no === receipt.pid);
  assert.equal(saved.board_flag, 'UN'); assert.equal(saved.flag_name, 'United Nations');
  assert.equal(Object.hasOwn(saved, 'country'), false);
  assert.deepEqual(errors, []); await active.close();
  console.log('Flags: script-free geographic post, ignored headers, real Quick Reply and updater board flag passed.');
} finally { await browser.close(); }
