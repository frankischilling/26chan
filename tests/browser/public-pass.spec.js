import { test, expect } from '@playwright/test';

test.skip(process.env.VERIFICATION_BROWSER_FIXTURE !== '1', 'Uses rendering-only Pass fixtures.');

for (const javaScriptEnabled of [true, false]) {
  test.describe(`Pass native forms, scripts ${javaScriptEnabled ? 'enabled' : 'disabled'}`, () => {
    test.use({ javaScriptEnabled });
    for (const width of [1280, 390]) {
      test(`credential form and explicit logout at ${width}px`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        const response = await page.goto('/fixture/pass/login');
        expect(response.headers()['cache-control']).toBe('private, no-store');
        expect(response.headers()['content-security-policy']).toContain("frame-ancestors 'none'");
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
        await expect(page.getByRole('link')).toHaveAttribute('href', '/auth');
      }
    });
  });
}
