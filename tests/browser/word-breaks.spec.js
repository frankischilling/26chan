import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000', password = 'owned-word-break-browser-password';
for (const javaScriptEnabled of [false, true]) {
  test(`source word breaks persist and enter the live thread (JavaScript ${javaScriptEnabled})`, async ({ browser }, info) => {
    const context = await browser.newContext({ javaScriptEnabled });
    const page = await context.newPage(); let op;
    const long = 'x'.repeat(70), destination = `https://example.org/${'a'.repeat(70)}`;
    try {
      await page.goto(`${origin}/test/`);
      if (javaScriptEnabled) await page.locator('#togglePostFormLink a').click();
      await page.locator('#sub').fill('Owned word breaks');
      await page.locator('#com').fill(`[b]${long}[/b]\n${'界'.repeat(35)}\n${destination}\nleft{{w_br}}right <script>`);
      await page.locator('#password').fill(password);
      await page.getByRole('button', { name: 'Post', exact: true }).click();
      await expect(page).toHaveURL(/\/test\/thread\/\d+#p\d+$/);
      op = /#p(\d+)$/.exec(page.url())[1];
      const message = page.locator(`#m${op}`);
      await expect(message.locator('.mu-s')).toHaveText(long);
      await expect(message.locator('.mu-s wbr')).toHaveCount(2);
      await expect(message.locator('a:not(.quotelink)')).toHaveAttribute('href', destination);
      await expect(message.locator('a wbr')).toHaveCount(2);
      await expect(message.locator('script')).toHaveCount(0);
      await expect(message).toContainText('leftright <script>');
      for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 900 });
        expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
        const path = info.outputPath(`word-breaks-${javaScriptEnabled}-${width}.png`);
        await page.locator(`#p${op}`).screenshot({ path });
        await info.attach(`word breaks ${width}`, { path, contentType: 'image/png' });
      }
      const reply = `[b]${'y'.repeat(70)}[/b]`;
      if (javaScriptEnabled) {
        await page.locator(`#p${op} .postInfo > .postNum`).click();
        await page.locator('#qrCom').fill(reply);
        await page.locator('#qr-pwd').fill(password);
        const before = page.url();
        await page.locator('#quickReply input[type=submit]').click();
        await expect(page.locator('.reply .mu-s wbr')).toHaveCount(2);
        expect(page.url()).toBe(before);
      } else {
        await page.locator('#com').fill(reply);
        await page.locator('#password').fill(password);
        await page.getByRole('button', { name: 'Post', exact: true }).click();
        await expect(page.locator('.reply .mu-s wbr')).toHaveCount(2);
      }
      await page.reload();
      await expect(page.locator('.reply .mu-s')).toHaveText('y'.repeat(70));
      await expect(page.locator('.reply .mu-s wbr')).toHaveCount(2);
      const json = await (await context.request.get(`${origin}/test/thread/${op}.json`)).json();
      expect(json.posts[1].com).toContain(`${'y'.repeat(35)}<wbr>${'y'.repeat(35)}<wbr>`);
      await page.goto(`${origin}/test/catalog?teaser=on`);
      await expect(page.locator(`#thread-${op} .teaser`)).toContainText(long);
      await expect(page.locator(`#thread-${op} .teaser`)).not.toContainText('{{w_br}}');
    } finally {
      try { if (op) expect((await context.request.post(`${origin}/test/delete`, { headers: { origin }, form: { no: op, password }, maxRedirects: 0 })).status()).toBe(303); }
      finally { await context.close(); }
    }
  });
}
