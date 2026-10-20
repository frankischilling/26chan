// Actual public/private verification on a disposable service. Inputs are owned
// synthetic credentials supplied on stdin; no database authority reaches Chrome.
import assert from 'node:assert/strict';
import { chromium, expect } from '@playwright/test';
let input = '';
for await (const chunk of process.stdin) { input += chunk; assert.ok(input.length <= 4096); }
const { origin, board, target, credential } = JSON.parse(input);
const address = new URL(origin);
assert.equal(address.protocol, 'http:'); assert.equal(address.hostname, '127.0.0.1');
assert.match(board, /^vp[0-9a-f]{8}$/); assert.match(target, /^[1-9][0-9]{0,18}$/);
assert.equal(credential, 'capcode_!owned-public-vip!owned-vip-secret');
for (const name of Object.keys(process.env)) assert.ok(!name.endsWith('DATABASE_URL') && !/TOKEN|KEY/.test(name));
let browser, stage = 'launch';
const ids = [];
let cancelled = false;
const cancel = () => { cancelled = true; void browser?.close().catch(() => {}); };
process.once('SIGTERM', cancel); process.once('SIGINT', cancel);
const lifetime = setTimeout(() => { stage = 'deadline'; cancel(); }, 60_000);
try {
  browser = await chromium.launch({ timeout: 15000 });
  assert.ok(!cancelled, 'owned browser cancelled');
  for (const mode of ['native', 'desktop-qr', 'mobile-qr']) {
    stage = `${mode}-open`;
    const context = await browser.newContext({ javaScriptEnabled: mode !== 'native', viewport: mode === 'mobile-qr' ? { width: 390, height: 844 } : { width: 1280, height: 900 } });
    await context.route('**/*', route => new URL(route.request().url()).origin === origin ? route.continue() : route.abort());
    if (mode !== 'native') await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true })));
    const page = await context.newPage(); page.setDefaultTimeout(12000);
    await page.goto(`${origin}/${board}/thread/${target}`);
    let id;
    if (mode === 'native') {
      const form = page.locator('form.postEditor').first();
      await form.locator('[name=email]').fill(credential);
      await form.locator('[name=com]').fill('Owned verified browser duplicate control');
      stage = `${mode}-submit`;
      await Promise.all([page.waitForURL(url => /^#p[1-9][0-9]*$/.test(url.hash)), form.getByRole('button', { name: 'Post', exact: true }).click()]);
      id = new URL(page.url()).hash.slice(2);
    } else {
      if (mode === 'mobile-qr') await page.locator(`#pim${target} a[title="Reply to this post"]`).click();
      else await page.locator('.open-qr-link').click();
      const qr = page.locator('#quickReply');
      await qr.locator('#qrEmail').fill(credential);
      await qr.locator('#qrCom').fill('Owned verified browser duplicate control');
      stage = `${mode}-submit`;
      const pending = page.waitForResponse(response => response.request().method() === 'POST' && new URL(response.url()).pathname === `/${board}/imgboard.php`);
      await qr.locator('input[type=submit]').click();
      const response = await pending;
      assert.equal(response.status(), 200);
      const result = await response.json(); assert.equal(result.tid, Number(target));
      assert.ok(Number.isSafeInteger(result.pid) && result.pid > 0); id = String(result.pid);
      await expect(qr.locator('#qrCom')).toHaveValue('');
      const stored = await page.evaluate(() => JSON.stringify({ local: { ...localStorage }, session: { ...sessionStorage } }));
      assert.ok(!stored.includes('owned-public-vip') && !stored.includes('owned-vip-secret'));
      await page.goto(`${origin}/${board}/thread/${target}#p${id}`);
    }
    stage = `${mode}-saved-header`;
    const post = page.locator(`#p${id}`);
    const header = post.locator(mode === 'mobile-qr' ? '.postInfoM' : '.postInfo');
    await expect(header.locator('.name')).toHaveText('Owned Verified');
    await expect(header.locator('.capcode')).toHaveText('## Verified');
    await expect(header.locator('.capcode')).toHaveAttribute('title', 'Highlight posts by Verified Users');
    await expect(header.locator('.capcode')).toHaveCSS('color', 'rgb(0, 127, 255)');
    assert.equal(await post.locator('.identityIcon').count(), 0);
    assert.ok(!(await post.getAttribute('class')).includes('highlightPost'));
    stage = `${mode}-preferences`;
    const html = await page.content();
    assert.ok(!html.includes('owned-public-vip') && !html.includes('owned-vip-secret'));
    for (const cookie of await context.cookies()) assert.ok(!cookie.value.includes('owned-public-vip') && !cookie.value.includes('owned-vip-secret'));
    await expect(page.locator('form.postEditor [name=email]').first()).toHaveValue('');
    ids.push(Number(id));
    await context.close();
  }
  assert.equal(new Set(ids).size, 3);
  process.stdout.write(JSON.stringify(ids));
} catch (error) {
  const where = String(error?.stack ?? '').match(/vip-posting\.mjs:([0-9]{1,6}):([0-9]{1,6})/);
  console.error(`OWNED_VIP_BROWSER stage=${stage}${where ? ` line=${where[1]}:${where[2]}` : ''}`);
  process.exitCode = 1;
} finally { clearTimeout(lifetime); await browser?.close(); }
