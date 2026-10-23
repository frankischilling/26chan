import assert from 'node:assert/strict';
import { passWritePacer } from './pass-state-pacing.mjs';
import { observeOwnedUploadResponse } from './owned-upload-response.mjs';
import { mkdir } from 'node:fs/promises';
const mode = process.argv[2];
assert.ok(['http', 'browser', 'noscript'].includes(mode));
let input = '';
for await (const part of process.stdin) { input += part; assert.ok(input.length < 32768); }
const fixture = JSON.parse(input);
for (const name of Object.keys(process.env)) {
  assert.ok(!name.endsWith('DATABASE_URL') && !name.endsWith('_KEY') && !name.endsWith('_TOKEN'), 'client inherited private authority');
}
for (const value of [fixture.origin, fixture.api]) {
  const url = new URL(value); assert.equal(url.protocol, 'http:'); assert.equal(url.hostname, '127.0.0.1');
}
assert.match(fixture.board, /^pm[a-f0-9]{8}$/);
const hidden = (html, name) => new RegExp(`name="${name}" value="([^"]*)"`).exec(html)?.[1] ?? '';
const api = async thread => {
  const response = await fetch(`${fixture.api}/${fixture.board}/thread/${thread}.json`, { signal: AbortSignal.timeout(10000) });
  assert.equal(response.status, 200); return response.json();
};
const safe = value => {
  const text = JSON.stringify(value);
  for (const name of ['pass_id', 'identity_hash', 'pin_hash', 'credential_version', 'recipient_hash', 'encrypted_contact']) assert.ok(!text.includes(`"${name}"`));
};
const posted = [];
const anonymous = [];
async function httpCase(row) {
  const cookies = new Map();
  async function request(path, fields, accept = 'text/html') {
    const headers = { Cookie: [...cookies].map(([k,v]) => `${k}=${v}`).join('; '), Accept: accept };
    if (fields) { headers.Origin = fixture.origin; headers['Content-Type'] = 'application/x-www-form-urlencoded'; }
    const response = await fetch(new URL(path, fixture.origin), { method: fields ? 'POST' : 'GET', headers, body: fields && new URLSearchParams(fields), redirect: 'manual', signal: AbortSignal.timeout(10000) });
    for (const cookie of response.headers.getSetCookie()) {
      const pair = cookie.split(';')[0], at = pair.indexOf('=');
      if (/max-age=0(?:;|$)/i.test(cookie)) cookies.delete(pair.slice(0,at));
      else { cookies.set(pair.slice(0,at), pair.slice(at+1)); if (pair.slice(0,at) === 'board-anon') anonymous.push(pair.slice(at+1)); }
    }
    return { status: response.status, text: await response.text(), headers: response.headers };
  }
  const form = await request('/auth'); assert.equal(form.status, 200);
  const login = await request('/auth', { csrf: hidden(form.text, 'csrf'), id: row.identifier, pin: row.pin });
  assert.equal(login.status, 200);
  assert.equal(cookies.has('board-pass'), row.active, `${row.name}: login entitlement`);
  if (!row.active) cookies.set('pass_enabled', '1');
  const page = await request(`/${fixture.board}/`); assert.equal(page.status, 200);
  const fields = { mode: 'regist', resto: '0', name: 'Anonymous', sub: `Owned ${row.name}`, com: `Owned Pass ${row.name} native`, email: 'since4pass', pwd: 'owned-state-password', 'captcha-csrf': hidden(page.text, 'captcha-csrf') };
  const result = await request(`/${fixture.board}/post`, fields, 'application/json');
  if (!row.active) {
    if (result.text.startsWith('{')) { const denied = JSON.parse(result.text); assert.ok(denied.error, `${row.name}: inactive account posted`); assert.equal(denied.pid, undefined); }
    else { assert.ok(result.status >= 400, `${row.name}: unexpected successful non-JSON write`); assert.match(result.text, /CAPTCHA|Pass/); }
  } else {
    assert.ok(result.text.startsWith('{'), `${row.name}: posting returned ${result.status}: ${result.text.slice(0,1600)}`);
    const created = JSON.parse(result.text);
    assert.equal(created.error, undefined, `${row.name}: ${created.error}`); assert.ok(created.pid > 0);
    posted.push({ id: created.pid, account: row.account, year: 2020 });
    const data = await api(created.pid); safe(data); assert.equal(data.posts[0].since4pass, 2020);
    const thread = await request(`/${fixture.board}/thread/${created.pid}`);
    assert.equal((thread.text.match(/title="Pass user since 2020" class="n-pu"/g) ?? []).length, 2);
    const reply = await request(`/${fixture.board}/imgboard.php`, { ...fields, resto: String(created.pid), sub: '', com: `Owned Pass ${row.name} JSON reply`, email: '', 'captcha-csrf': hidden(thread.text, 'captcha-csrf') }, 'application/json');
    const saved = JSON.parse(reply.text); assert.equal(saved.error, undefined, `${row.name}: ${saved.error}`); assert.equal(saved.tid, created.pid);
    posted.push({ id: saved.pid, account: row.account, year: 0 });
    const updated = await api(created.pid); safe(updated); assert.equal(updated.posts.length, 2); assert.equal(updated.posts[1].since4pass, undefined);
  }
  if (cookies.has('board-anon')) anonymous.push(cookies.get('board-anon'));
}
async function browserCases() {
  const { chromium, expect } = await import('@playwright/test');
  const pacing = passWritePacer();
  await mkdir('test-results/pass-state-matrix', { recursive: true });
  const browser = await chromium.launch({ timeout: 15000 });
  const stop = () => { void browser.close(); };
  process.once('SIGTERM', stop); process.once('SIGINT', stop);
  try {
    for (const [index,row] of fixture.cases.entries()) {
      await pacing.beginCase();
      const width = index % 2 ? 390 : 1280;
      const context = await browser.newContext({ javaScriptEnabled: mode !== 'noscript', viewport: { width, height: 900 } });
      context.on('request', request => { if (request.method() === 'POST') pacing.write(); });
      await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: true, quickReply: true })));
      const openQuick = async page => page.locator(width <= 480 ? '#mpostform .mobilePostFormToggle' : '.open-qr-link').click();
      const page = await context.newPage(); page.setDefaultTimeout(12000);
      try {
        await page.goto(`${fixture.origin}/auth`);
        await page.locator('#field-id').fill(row.identifier); await page.locator('#field-pin').fill(row.pin);
        await page.locator('#auth-btn').click();
        if (row.active) await expect(page.locator('.msg-success')).toContainText('Success!');
        else {
          await expect(page.locator('.msg-error')).toBeVisible();
          await context.addCookies([{ name: 'pass_enabled', value: '1', url: fixture.origin }]);
        }
        // The native submit guard rejects a click while Pass advice is loading.
        // Observing the layout or the server nonce does not mean that this
        // asynchronous request has finished, especially on a busy CI runner.
        const advice = row.active && mode !== 'noscript'
          ? page.waitForResponse(response => {
            const request = response.request();
            if (response.url() !== `${fixture.origin}/captcha` || request.method() !== 'POST') return false;
            const fields = new URLSearchParams(request.postData() ?? '');
            return fields.get('board') === fixture.board && fields.get('target') === '0' && fields.get('purpose') === 'thread';
          }, { timeout: 12000 }) : null;
        // Keep navigation failures from leaving a rejected observer unhandled.
        advice?.catch(() => {});
        await page.goto(`${fixture.origin}/${fixture.board}/`);
        if (advice) {
          const response = await advice;
          assert.equal(response.status(), 200);
          const result = await response.json();
          assert.equal(result.challenge.state, 'disabled');
          assert.equal(result.report_exempt, false);
          const captcha = page.locator('form.postEditor [data-posting-captcha]').first();
          await expect(captcha.locator('[name="captcha-csrf"]')).toHaveValue(result.csrf);
          await expect(captcha.locator('.captchaMessage')).toHaveText('');
          await expect(captcha.locator('button[id$="t-load"]')).toBeEnabled();
          await expect(captcha).toBeHidden();
        }
        if (!row.active) {
          // A presentation cookie cannot authorize a native or JSON write.
          const csrf = await page.locator('form.postEditor [name="captcha-csrf"]').first().inputValue();
          pacing.write();
          const denied = await context.request.post(`${fixture.origin}/${fixture.board}/post`, { headers: { Origin: fixture.origin, Accept: 'application/json' }, form: { com: 'Owned refused member post', email: 'since4pass', 'captcha-csrf': csrf }, maxRedirects: 0 });
          if (denied.headers()['content-type']?.startsWith('application/json')) { const value = await denied.json(); assert.ok(value.error); assert.equal(value.pid, undefined); }
          else { assert.ok(denied.status() >= 400); assert.match(await denied.text(), /CAPTCHA|Pass/); }
          if (mode === 'noscript') continue;
          await page.goto(`${fixture.origin}/${fixture.board}/thread/${fixture.target}`);
          await openQuick(page);
          await expect(page.locator('#quickReply')).toBeVisible();
          await expect(page.locator('#quickReply [data-posting-captcha]')).toBeVisible();
          continue;
        }
        const form = page.locator('form.postEditor').first();
        if (mode !== 'noscript') {
          await expect(form).toHaveClass(/nativePostForm/);
          await page.locator(width <= 480 ? '#mpostform .mobilePostFormToggle' : '#togglePostFormLink a').click();
        }
        await form.locator('[name="sub"]').fill(`Owned ${row.name}`);
        await form.locator('[name="com"]').fill(`Owned Pass ${row.name} native`);
        await form.locator('[name="email"]').fill('since4pass');
        await form.locator('button[type="submit"]').click();
        await expect(page).toHaveURL(new RegExp(`/${fixture.board}/thread/[1-9][0-9]*#p[1-9][0-9]*$`));
        const thread = Number(/\/thread\/(\d+)/.exec(page.url())[1]);
        posted.push({ id: thread, account: row.account, year: 2020 });
        await expect(page.locator(`#pc${thread} .n-pu`)).toHaveCount(2);
        await expect(page.locator(`#pc${thread} .postInfo .n-pu`)).toHaveAttribute('title', 'Pass user since 2020');
        const badge = page.locator(`#pc${thread} ${width <= 480 ? '.postInfoM' : '.postInfo'} .n-pu`);
        const badgeBox = await badge.boundingBox();
        assert.equal(badgeBox.width, 16); assert.equal(badgeBox.height, 16);
        if (mode === 'noscript') {
          const replyForm = page.locator('form.postEditor').first();
          await replyForm.locator('[name="com"]').fill(`Owned Pass ${row.name} native reply`);
          await replyForm.locator('[name="email"]').fill('since4pass');
          await replyForm.locator('button[type="submit"]').click();
          await expect(page).toHaveURL(new RegExp(`/${fixture.board}/thread/${thread}#p[1-9][0-9]*$`));
          const reply = Number(/#p(\d+)$/.exec(page.url())[1]); assert.notEqual(reply, thread);
          posted.push({ id: reply, account: row.account, year: 2020 });
          const data = await api(thread); safe(data); assert.equal(data.posts.length,2);
          assert.equal(data.posts[0].since4pass,2020); assert.equal(data.posts[1].since4pass,2020);
          continue;
        }
        await openQuick(page);
        const qr = page.locator('#quickReply'); await expect(qr).toBeVisible();
        await expect(qr.locator('[name="captcha-csrf"]')).toHaveValue(/^[a-f0-9]{64}$/);
        await expect(qr.locator('[data-posting-captcha]')).toBeHidden();
        await qr.locator('[name="com"]').fill(`Owned Pass ${row.name} Quick Reply`);
        await qr.locator('[name="email"]').fill('since4pass');
        const response = await observeOwnedUploadResponse(page, `${fixture.origin}/${fixture.board}/imgboard.php`, 'post');
        await qr.locator('input[type="submit"]').click();
        const { status, result: saved } = await response();
        assert.equal(status, 200); assert.equal(saved.error, undefined); assert.equal(saved.tid, thread);
        posted.push({ id: saved.pid, account: row.account, year: 2020 });
        await expect(qr.locator('[name="com"]')).toHaveValue('');
        const data = await api(thread); safe(data); assert.equal(data.posts.length, 2);
        assert.equal(data.posts[0].since4pass, 2020); assert.equal(data.posts[1].since4pass, 2020);
      } catch (error) {
        // Only owned fixture pages are captured; no database or issuer secrets
        // are available to this child. Preserve the original failure.
        if (/^[a-z_]+$/.test(row.name)) await page.screenshot({ path: `test-results/pass-state-matrix/${mode}-${row.name}-${width}.png`, fullPage: true }).catch(() => {});
        throw error;
      } finally {
        const cookie = (await context.cookies()).find(cookie => cookie.name === 'board-anon'); if (cookie) anonymous.push(cookie.value);
        await context.close();
        pacing.endCase();
      }
    }
  } finally { await browser.close(); }
}
try {
  if (mode === 'http') for (const row of fixture.cases) await httpCase(row);
  else await browserCases();
} catch (error) { console.error(error); process.exitCode = 1; }
finally { console.log(JSON.stringify({ posted, anonymous: [...new Set(anonymous)] })); }
