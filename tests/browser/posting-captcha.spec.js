import { test, expect } from '@playwright/test';
import { readFile, mkdir } from 'node:fs/promises';
test.skip(process.env.VERIFICATION_BROWSER_FIXTURE !== '1', 'Uses the explicit CAPTCHA UI fixture.');
const images = await Promise.all(['foreground', 'background'].map(async name => 'data:image/png;base64,' + (await readFile(new URL(`../fixtures/verification-render/${name}.png`, import.meta.url))).toString('base64')));
const nonce = 'ab'.repeat(32), token = `c1_${'cd'.repeat(32)}`, ticket = `t1_${'ef'.repeat(32)}`;
const reply = challenge => ({ csrf: nonce, ticket_challenge: false, report_exempt: false, challenge });
const ready = () => reply({ state: 'ready', token, image: images[0], background: images[1], slider: true, offset_max: 80, ttl_seconds: 120, cooldown_seconds: 5 });
async function mock(page, responses = [ready()]) {
  const calls = [];
  await page.route('**/captcha', async route => {
    calls.push(new URLSearchParams(route.request().postData()));
    await route.fulfill({ contentType: 'application/json', body: JSON.stringify(responses[Math.min(calls.length - 1, responses.length - 1)]) });
  });
  return calls;
}
for (const mobile of [false, true]) {
  test(`posting CAPTCHA image, slider and scoped CSP ${mobile ? 'mobile' : 'desktop'}`, async ({ page }) => {
    await page.setViewportSize({ width: mobile ? 390 : 1280, height: 900 });
    await page.addInitScript(() => { window.violations = []; document.addEventListener('securitypolicyviolation', e => violations.push(e.violatedDirective)); });
    const calls = await mock(page), response = await page.goto('/fixture/captcha/reply');
    expect(response.headers()['content-security-policy']).toContain('connect-src http://127.0.0.1:3000/captcha;');
    expect(calls).toHaveLength(0);
    await page.getByRole('button', { name: 'Get Captcha', exact: true }).click();
    await expect(page.locator('#t-fg')).toHaveJSProperty('naturalWidth', 240);
    await expect(page.locator('#t-bg')).toHaveJSProperty('naturalWidth', 320);
    expect(Object.fromEntries(calls[0])).toEqual({ action: 'init', board: 'g', target: '10', purpose: 'reply' });
    await page.getByRole('slider', { name: 'Align the image' }).focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('#t-bg')).toHaveCSS('transform', 'matrix(1, 0, 0, 1, -0.8, 0)');
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    expect(await page.evaluate(() => violations)).toEqual([]);
    await mkdir('.local/verification-review', { recursive: true });
    await page.screenshot({ path: `.local/verification-review/posting-${mobile ? 'mobile' : 'desktop'}.png`, fullPage: true });
  });
}
test('native submission includes proof without exposing private scope', async ({ page }) => {
  await mock(page); await page.goto('/fixture/captcha/thread');
  await page.locator('#fixture-comment').fill('Owned fixture post');
  await page.getByRole('button', { name: 'Submit', exact: true }).click();
  await expect(page.locator('#t-msg')).toContainText('Complete the CAPTCHA');
  await page.getByRole('button', { name: 'Get Captcha', exact: true }).click();
  await expect(page.locator('#t-resp')).toBeVisible(); await page.locator('#t-resp').fill('ABC123');
  let submitted;
  await page.route('**/fixture/captcha/submission', async route => { submitted = new URLSearchParams(route.request().postData()); await route.fulfill({ contentType: 'text/html', body: '<p>Owned submission received.</p>' }); });
  await page.getByRole('button', { name: 'Submit', exact: true }).click();
  await expect(page.getByText('Owned submission received.')).toBeVisible();
  expect(submitted.get('captcha-csrf')).toBe(nonce); expect(submitted.get('t-challenge')).toBe(token); expect(submitted.get('t-response')).toBe('ABC123');
  expect([...submitted.keys()].sort()).toEqual(['captcha-credit', 'captcha-csrf', 'com', 'resto', 't-challenge', 't-response']);
});
test('ticket completion waits, resumes a challenge, and never auto-posts', async ({ page }) => {
  await page.clock.install(); const first = ready(); first.ticket_challenge = true;
  const calls = await mock(page, [first, reply({ state: 'wait', seconds: 10, ticket }), ready()]);
  await page.goto('/fixture/captcha/reply');
  let posts = 0; await page.route('**/fixture/captcha/submission', route => { posts++; return route.abort(); });
  await page.getByRole('button', { name: 'Get Captcha', exact: true }).click();
  await expect(page.locator('#t-msg')).toContainText('waiting period');
  await page.locator('#t-resp').fill('ABCDEFGH'); await page.clock.fastForward(5000);
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(page.locator('#t-load')).toHaveText('Wait 10s');
  expect(calls[1].get('ticket_answer')).toBe('ABCDEFGH'); expect(calls[1].get('ticket_challenge')).toBe(token);
  await expect(page.locator('#t-challenge')).toHaveValue('');
  expect(await page.evaluate(() => localStorage.getItem('26chan:captcha-ticket:g'))).toBe(ticket);
  await page.clock.fastForward(10000); await expect(page.locator('#t-challenge')).toHaveValue(token);
  expect(calls[2].get('ticket')).toBe(ticket); expect(posts).toBe(0);
});
test('credits are informational and expire; restored pages clear stale proof', async ({ page }) => {
  await page.clock.install(); await mock(page, [reply({ state: 'credits', remaining: 2, ttl_seconds: 120 })]);
  await page.goto('/fixture/captcha/reply'); await page.locator('#t-load').click();
  await expect(page.locator('#t-msg')).toHaveText('Verification not required.');
  await expect(page.locator('[name="captcha-credit"]')).toHaveValue('1');
  expect(await page.locator('#t-root input[type="checkbox"]').count()).toBe(0);
  await page.clock.fastForward(117000); await expect(page.locator('[name="captcha-credit"]')).toHaveValue('');
  await expect(page.locator('#t-msg')).toContainText('expired');
  await page.locator('#t-load').click(); await expect(page.locator('[name="captcha-credit"]')).toHaveValue('1');
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  await expect(page.locator('[name="captcha-credit"]')).toHaveValue(''); await expect(page.locator('[name="captcha-csrf"]')).toHaveValue('');
});
test('report exemption hides only the controls and retains the form nonce', async ({ page }) => {
  const calls = await mock(page, [{ ...reply({ state: 'disabled' }), report_exempt: true }]);
  await page.goto('/fixture/captcha/report'); await page.locator('#t-load').click();
  await expect(page.locator('#t-root')).toBeHidden(); await expect(page.locator('[name="captcha-csrf"]')).toHaveValue(nonce);
  expect(calls[0].get('purpose')).toBe('report'); expect(calls[0].get('target')).toBe('10');
});
for (const mode of ['classic', 'noscript']) {
  test(`${mode} server-rendered form submits with JavaScript disabled`, async ({ browser }) => {
    const context = await browser.newContext({ javaScriptEnabled: false }); const page = await context.newPage();
    try {
      await page.goto(`http://127.0.0.1:3000/fixture/captcha/${mode}`);
      await expect(page.locator('#t-fg')).toBeVisible(); await expect(page.locator('#t-challenge')).toHaveValue(token);
      await page.locator('#fixture-comment').fill('Owned classic post'); await page.locator('#t-resp').fill('ABC123');
      let submitted; await page.route('**/fixture/captcha/submission', async route => { submitted = new URLSearchParams(route.request().postData()); await route.fulfill({ contentType: 'text/html', body: '<p>Native classic submission.</p>' }); });
      await page.getByRole('button', { name: 'Submit', exact: true }).click(); await expect(page.getByText('Native classic submission.')).toBeVisible();
      expect(submitted.get('captcha-csrf')).toBe(nonce); expect(submitted.get('t-response')).toBe('ABC123');
    } finally { await context.close(); }
  });
}
test('network failure clears proof and delays manual retry without auto-posting', async ({ page }) => {
  await page.clock.install(); let calls = 0;
  await page.route('**/captcha', async route => {
    calls++;
    if (calls === 1) await route.abort('failed');
    else await route.fulfill({ contentType: 'application/json', body: JSON.stringify(ready()) });
  });
  await page.goto('/fixture/captcha/reply'); await page.locator('#t-load').click();
  await expect(page.locator('#t-msg')).toContainText('Could not load');
  await expect(page.locator('#t-challenge')).toHaveValue(''); await expect(page.locator('#t-load')).toBeDisabled();
  await page.clock.fastForward(60000); await expect(page.locator('#t-load')).toBeEnabled(); expect(calls).toBe(1);
  await page.locator('#t-load').click(); await expect(page.locator('#t-challenge')).toHaveValue(token); expect(calls).toBe(2);
});
test('a disabled board hides controls without inventing a receipt or credit', async ({ page }) => {
  await mock(page, [reply({ state: 'disabled' })]); await page.goto('/fixture/captcha/thread'); await page.locator('#t-load').click();
  await expect(page.locator('#t-root')).toBeHidden(); await expect(page.locator('#t-challenge')).toHaveValue('');
  await expect(page.locator('[name="captcha-credit"]')).toHaveValue(''); await expect(page.locator('[name="captcha-csrf"]')).toHaveValue(nonce);
});
async function openQuickReply(page, cooldown = false) {
  await page.goto('/fixture/captcha/qr');
  await page.evaluate(async cooldown => {
    if (cooldown) {
      document.querySelector('form.postEditor').dataset.postingReplySeconds = '10';
      localStorage.setItem('4chan-cd-g', String(Date.now()));
    }
    const { mountNativeQuickReply } = await import('/static/native-quick-reply.v1.js');
    window.fixtureQR = mountNativeQuickReply({ board: 'g', thread: '10', settings: () => ({ quickReply: true, persistentQR: true }) });
    window.fixtureQR.open();
  }, cooldown);
  await expect(page.locator('#quickReply')).toBeVisible();
}
test('Quick Reply scopes proofs to the current thread and clears them after a real submission attempt', async ({ page }) => {
  const calls = await mock(page); await openQuickReply(page);
  await page.locator('#qr-t-load').click(); await expect(page.locator('#qr-t-challenge')).toHaveValue(token);
  expect(calls[0].get('target')).toBe('10');
  await page.locator('#pi11 a[title="Reply to this post"]').click();
  await expect(page.locator('#qrResto')).toHaveValue('11'); await expect(page.locator('#qr-t-challenge')).toHaveValue('');
  await page.locator('#qr-t-load').click(); await expect(page.locator('#qr-t-challenge')).toHaveValue(token);
  expect(calls[1].get('target')).toBe('11');
  await page.locator('#qrCom').fill('Owned QR attempt'); await page.locator('#qr-t-resp').fill('ABC123');
  let body;
  await page.route('**/g/imgboard.php', async route => { body = route.request().postData(); await route.fulfill({ status: 400, contentType: 'application/json', body: JSON.stringify({ error: 'Owned rejection.' }) }); });
  await page.locator('#quickReply input[type="submit"]').click();
  await expect(page.locator('#qrError')).toContainText('Owned rejection.');
  expect(body).toContain(token); expect(body).toContain('ABC123');
  await expect(page.locator('#qr-t-challenge')).toHaveValue('');
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  await expect(page.locator('#quickReply')).toHaveCount(0);
});
test('Quick Reply close fences an in-flight CAPTCHA response', async ({ page }) => {
  let release; const gate = new Promise(resolve => { release = resolve; }); let started = false;
  await page.route('**/captcha', async route => { started = true; await gate; await route.fulfill({ contentType: 'application/json', body: JSON.stringify(ready()) }).catch(() => {}); });
  await openQuickReply(page); await page.locator('#qr-t-load').click(); await expect.poll(() => started).toBe(true);
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click(); release();
  await page.evaluate(() => window.fixtureQR.open());
  await expect(page.locator('#qr-t-challenge')).toHaveValue(''); await expect(page.locator('#qr-t-frame')).toBeHidden();
});
test('hydrated classic controls keep the native image and expire without requesting a replacement', async ({ page }) => {
  await page.clock.install(); const calls = await mock(page);
  await page.goto('/fixture/captcha/classic'); await expect(page.locator('#t-challenge')).toHaveValue(token);
  await expect(page.locator('#t-slider')).toBeHidden(); await expect(page.locator('#t-bg')).toBeHidden();
  await page.locator('#t-resp').fill('ABC123'); expect(calls).toHaveLength(0);
  await page.clock.fastForward(117000); await expect(page.locator('#t-challenge')).toHaveValue('');
  await expect(page.locator('#t-resp')).toHaveValue(''); await expect(page.locator('#t-frame')).toBeHidden(); expect(calls).toHaveLength(0);
});
test('submitting a ticket-stage form completes only the ticket exchange', async ({ page }) => {
  await page.clock.install(); const first = ready(); first.ticket_challenge = true;
  const calls = await mock(page, [first, reply({ state: 'wait', seconds: 900, ticket })]);
  let posts = 0; await page.route('**/fixture/captcha/submission', route => { posts++; return route.abort(); });
  await page.goto('/fixture/captcha/thread'); await page.locator('#fixture-comment').fill('Held until ordinary challenge');
  await page.locator('#t-load').click(); await expect(page.locator('#t-resp')).toBeVisible();
  await page.locator('#t-resp').fill('ABCDEFGH'); await page.clock.fastForward(5000);
  await page.getByRole('button', { name: 'Submit', exact: true }).click();
  await expect(page.locator('#t-load')).toHaveText('Wait 900s'); expect(calls).toHaveLength(2); expect(posts).toBe(0);
  expect(calls[1].get('ticket_answer')).toBe('ABCDEFGH'); await expect(page.locator('#t-challenge')).toHaveValue('');
});

test('CAPTCHA refresh countdown preserves an explicitly armed Quick Reply cooldown', async ({ page }) => {
  await page.clock.install(); await mock(page); await openQuickReply(page, true);
  let posts = 0;
  await page.route('**/g/imgboard.php', async route => { posts++; await route.fulfill({ status: 400, contentType: 'application/json', body: JSON.stringify({ error: 'Owned cooldown submission.' }) }); });
  await page.locator('#qr-t-load').click(); await expect(page.locator('#qr-t-challenge')).toHaveValue(token);
  await page.locator('#qrCom').fill('Owned explicitly armed reply'); await page.locator('#qr-t-resp').fill('ABC123');
  const submit = page.locator('#quickReply input[type="submit"]'); await submit.click();
  await expect(submit).toHaveValue(/ \(auto\)$/);
  await page.clock.fastForward(1000); await expect(submit).toHaveValue(/ \(auto\)$/); expect(posts).toBe(0);
  await page.clock.fastForward(9000); await expect(page.locator('#qrError')).toContainText('Owned cooldown submission.'); expect(posts).toBe(1);
});
