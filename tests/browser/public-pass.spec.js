import { test, expect } from '@playwright/test';

test.skip(process.env.VERIFICATION_BROWSER_FIXTURE !== '1', 'Uses rendering-only Pass fixtures.');

for (const javaScriptEnabled of [true, false]) {
  test.describe(`Pass native forms, scripts ${javaScriptEnabled ? 'enabled' : 'disabled'}`, () => {
    test.use({ javaScriptEnabled });
    for (const width of [1280, 390]) {
      test(`credential form and explicit logout at ${width}px`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        const response = await page.goto('/fixture/pass/login');
        await expect(page).toHaveTitle('4chan Pass - Authenticate');
        await expect(page.locator('#title')).toHaveText('4chan Pass');
        expect(response.headers()['cache-control']).toBe('private, no-store');
        expect(response.headers()['content-security-policy']).toContain("frame-ancestors 'none'");
        await expect(page.getByRole('link', { name: 'Go here', exact: true })).toHaveAttribute('href', '/pass?reset');
        await expect(page.getByRole('link', { name: 'Click here', exact: true })).toHaveAttribute('href', '/pass');
        const token = page.getByLabel('Token', { exact: true });
        const pin = page.getByLabel('PIN', { exact: true });
        await expect(pin).toHaveAttribute('type', 'password');
        await expect(token).toHaveAttribute('maxlength', '10');
        await expect(page.getByLabel('Remember this device for 1 year')).not.toBeChecked();
        let sent;
        await page.route('**/auth', async route => {
          sent = route.request();
          await route.fulfill({ contentType: 'text/html', body: '<p>Owned form submission received.</p>' });
        });
        // Native validation works with scripts disabled too, without transmitting blanks.
        await page.getByRole('button', { name: 'Submit', exact: true }).click();
        expect(sent).toBeUndefined();
        await token.fill('OWNED12345');
        await pin.fill('owned-browser-pin');
        await page.getByLabel('Remember this device for 1 year').check();
        const csrf = await page.locator('input[name="csrf"]').inputValue();
        expect(csrf.length).toBeGreaterThan(20);
        await expect(page.locator('body')).not.toContainText('owned-browser-pin');
        const content = await page.locator('#content').boundingBox();
        expect(content.x).toBeGreaterThanOrEqual(0);
        expect(content.x + content.width).toBeLessThanOrEqual(width);
        await page.screenshot({ path: `.local/verification-review/pass-login-${width}-${javaScriptEnabled}.png`, fullPage: true });
        await page.getByRole('button', { name: 'Submit', exact: true }).click();
        await expect(page.getByText('Owned form submission received.')).toBeVisible();
        expect(sent.method()).toBe('POST');
        expect(sent.url()).toBe('http://127.0.0.1:3000/auth');
        expect(sent.headers().origin).toBe('http://127.0.0.1:3000');
        expect(Object.fromEntries(new URLSearchParams(sent.postData()))).toEqual({
          csrf, id: 'OWNED12345', pin: 'owned-browser-pin', long_login: '1',
        });
        await page.goto('/fixture/pass/authenticated');
        await expect(page.getByText('You are authenticated.')).toBeVisible();
        await expect(page.locator('input[name="pin"]')).toHaveCount(0);
        const logoutCsrf = await page.locator('input[name="csrf"]').inputValue();
        await page.getByRole('button', { name: 'Logout', exact: true }).click();
        await expect(page.getByText('Owned form submission received.')).toBeVisible();
        expect(Object.fromEntries(new URLSearchParams(sent.postData()))).toEqual({ csrf: logoutCsrf, logout: '1' });
      });
    }
    test('completion, errors and escaped messages retain no credential controls', async ({ page }) => {
      for (const [mode, text] of [
        ['success', 'Success! Your device is now authorized.'],
        ['logout', 'You are now logged out.'],
        ['error', 'Incorrect Token or PIN.'],
        ['escaped', '<script>owned fixture</script>'],
      ]) {
        await page.goto(`/fixture/pass/${mode}`);
        await expect(mode === 'error' || mode === 'escaped'
          ? page.getByRole('alert')
          : page.getByRole('heading', { level: 2 })).toHaveText(text);
        await expect(page.locator('input[name="pin"], input[name="id"], script')).toHaveCount(0);
        if (mode === 'error' || mode === 'escaped') await expect(page.locator('a[href="/auth"]')).toBeVisible();
        else await expect(page.locator('a[href="/auth"]')).toHaveCount(0);
      }
    });
  });
}

for (const javaScriptEnabled of [true, false]) {
  test.describe(`Pass recovery forms, scripts ${javaScriptEnabled ? 'enabled' : 'disabled'}`, () => {
    test.use({ javaScriptEnabled });
    for (const width of [1280, 390]) {
      test(`owner request and new PIN submission at ${width}px`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        const response = await page.goto('/fixture/pass/recovery-request');
        expect(response.headers()['cache-control']).toBe('private, no-store');
        expect(response.headers()['referrer-policy']).toBe('strict-origin');
        let sent;
        await page.route('**/pass', async route => {
          sent = route.request();
          await route.fulfill({ contentType: 'text/html', body: '<p>Owned recovery submission received.</p>' });
        });
        await page.getByRole('button', { name: 'Send recovery link' }).click();
        expect(sent).toBeUndefined();
        await page.getByLabel('Token', { exact: true }).fill('OWNED12345');
        await page.getByLabel('Owner email').fill('owned+gift@example.org');
        const csrf = await page.locator('input[name="csrf"]').inputValue();
        const content = await page.locator('#content').boundingBox();
        expect(content.x).toBeGreaterThanOrEqual(0);
        expect(content.x + content.width).toBeLessThanOrEqual(width);
        await page.screenshot({ path: `.local/verification-review/pass-recovery-${width}-${javaScriptEnabled}.png`, fullPage: true });
        await page.getByRole('button', { name: 'Send recovery link' }).click();
        await expect(page.getByText('Owned recovery submission received.')).toBeVisible();
        expect(sent.method()).toBe('POST');
        expect(sent.headers().origin).toBe('http://127.0.0.1:3000');
        expect(Object.fromEntries(new URLSearchParams(sent.postData()))).toEqual({ action: 'request', csrf, id: 'OWNED12345', email: 'owned+gift@example.org' });
        await page.goto('/fixture/pass/recovery-reset');
        await expect(page.getByLabel('New PIN', { exact: true })).toHaveAttribute('type', 'password');
        await expect(page.getByLabel('Confirm PIN')).toHaveAttribute('type', 'password');
        await page.getByLabel('New PIN', { exact: true }).fill('owned-new-pin');
        await page.getByLabel('Confirm PIN').fill('owned-new-pin');
        const resetCsrf = await page.locator('input[name="csrf"]').inputValue();
        const token = await page.locator('input[name="token"]').inputValue();
        await expect(page.locator('body')).not.toContainText('owned-new-pin');
        await page.getByRole('button', { name: 'Change PIN' }).click();
        await expect(page.getByText('Owned recovery submission received.')).toBeVisible();
        expect(sent.headers().origin).toBe('http://127.0.0.1:3000');
        expect(Object.fromEntries(new URLSearchParams(sent.postData()))).toEqual({ action: 'complete', csrf: resetCsrf, token, pin: 'owned-new-pin', confirm: 'owned-new-pin' });
        await page.goto('/fixture/pass/recovery-escaped');
        await expect(page.getByRole('status')).toHaveText('<script>owned recovery fixture</script>');
        await expect(page.locator('script, input[name="pin"], input[name="token"]')).toHaveCount(0);
      });
    }
  });
}

for (const javaScriptEnabled of [true, false]) {
  test.describe(`Pass sandbox checkout, scripts ${javaScriptEnabled ? 'enabled' : 'disabled'}`, () => {
    test.use({ javaScriptEnabled });
    for (const width of [1280, 390]) {
      test(`purchase and delayed gift form works without payment scripts at ${width}px`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        await page.goto('/fixture/pass/checkout-purchase');
        await expect(page.getByRole('heading', { name: 'Purchase a Pass' })).toBeVisible();
        await expect(page.locator('script, iframe, input[autocomplete^="cc-"]')).toHaveCount(0);
        await page.locator('[name="email"]').fill('buyer@example.org');
        await page.locator('[name="email_confirm"]').fill('buyer@example.org');
        await page.locator('[name="gift_email"]').fill('gift@example.org');
        await page.locator('[name="gift_confirm"]').fill('gift@example.org');
        await page.locator('[name="delayed"]').check();
        const csrf = await page.locator('[name="csrf"]').inputValue();
        const bounds = await page.locator('#content').boundingBox();
        expect(bounds.x).toBeGreaterThanOrEqual(0);
        expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
        await page.screenshot({ path: `.local/verification-review/pass-purchase-${width}-${javaScriptEnabled}.png`, fullPage: true });
        let sent;
        await page.route('**/pass/purchase', async route => {
          sent = route.request();
          await route.fulfill({ contentType: 'text/html', body: '<p>Owned gift checkout request received.</p>' });
        });
        await page.getByRole('button', { name: 'Continue to sandbox checkout' }).click();
        await expect(page.getByText('Owned gift checkout request received.')).toBeVisible();
        expect(sent.method()).toBe('POST');
        expect(Object.fromEntries(new URLSearchParams(sent.postData()))).toEqual({ csrf, email: 'buyer@example.org', email_confirm: 'buyer@example.org', gift_email: 'gift@example.org', gift_confirm: 'gift@example.org', delayed: '1', amount_minor: '2000', currency: 'usd' });
      });
      test(`renewal quote uses native forms without card collection at ${width}px`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        const response = await page.goto('/fixture/pass/checkout-renew');
        expect(response.headers()['cache-control']).toBe('private, no-store');
        expect(response.headers()['referrer-policy']).toBe('strict-origin');
        await expect(page.getByText('20.00 USD', { exact: true })).toBeVisible();
        await expect(page.getByRole('status')).toContainText('No real payment');
        await expect(page.locator('script, iframe, input[type="password"], input[autocomplete^="cc-"]')).toHaveCount(0);
        const csrf = await page.locator('input[name="csrf"]').inputValue();
        const bounds = await page.locator('#content').boundingBox();
        expect(bounds.x).toBeGreaterThanOrEqual(0);
        expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
        await page.screenshot({ path: `.local/verification-review/pass-checkout-${width}-${javaScriptEnabled}.png`, fullPage: true });
        let sent;
        await page.route('**/pass/renew', async route => {
          sent = route.request();
          await route.fulfill({ contentType: 'text/html', body: '<p>Owned sandbox order request received.</p>' });
        });
        await page.getByRole('button', { name: 'Continue to sandbox checkout' }).click();
        await expect(page.getByText('Owned sandbox order request received.')).toBeVisible();
        expect(sent.method()).toBe('POST');
        expect(sent.headers().origin).toBe('http://127.0.0.1:3000');
        expect(Object.fromEntries(new URLSearchParams(sent.postData()))).toEqual({ csrf, reference: 'owned-renewal', amount_minor: '2000', currency: 'usd' });
      });
    }
    test('payment results do not offer a second submission or render untrusted HTML', async ({ page }) => {
      for (const [mode, text] of [
        ['pending', 'Sandbox payment pending'], ['paid', 'Payment Success'],
        ['declined', 'Payment Declined'], ['cancelled', 'Payment cancelled'],
        ['refunded', 'Payment refunded'], ['disputed', 'Payment disputed'],
        ['expired', 'Checkout expired'], ['escaped', '<script>owned checkout fixture</script>'],
      ]) {
        await page.goto(`/fixture/pass/checkout-${mode}`);
        await expect(page.getByRole('status')).toContainText(text);
        await expect(page.locator('form, script, iframe')).toHaveCount(0);
      }
    });
  });
}
