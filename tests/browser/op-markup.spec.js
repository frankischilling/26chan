import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000', password = 'owned-op-markup-browser-password';
for (const javaScriptEnabled of [false, true]) {
  test(`source OP markup persists through native posting and live replies (JavaScript ${javaScriptEnabled})`, async ({ browser }) => {
    const context = await browser.newContext({ javaScriptEnabled });
    const page = await context.newPage(); let op;
    try {
      await page.goto(`${origin}/test/`);
      if (javaScriptEnabled) await page.locator('#togglePostFormLink a').click();
      await page.locator('#sub').fill('Owned OP markup');
      await page.locator('#com').fill('[b]bold[/b] [red]red[/red]');
      await page.locator('#password').fill(password);
      await page.getByRole('button', { name: 'Post', exact: true }).click();
      await expect(page).toHaveURL(/\/test\/thread\/\d+#p\d+$/);
      op = /#p(\d+)$/.exec(page.url())[1];
      await expect(page.locator(`#m${op} .mu-s`)).toHaveText('bold');
      await expect(page.locator(`#m${op} .mu-r`)).toHaveCSS('color', 'rgb(196, 30, 58)');
      const comment = '[i]<img src=x onerror=bad()>[/i] [green]green[/green] [blue]blue[/blue]';
      if (javaScriptEnabled) {
        await page.locator(`#p${op} .postInfo > .postNum`).click();
        await page.locator('#qrCom').fill(comment);
        await page.locator('#qr-pwd').fill(password);
        const before = page.url();
        await page.locator('#quickReply input[type=submit]').click();
        await expect(page.locator('.postMessage .mu-g')).toHaveText('green');
        expect(page.url()).toBe(before);
      } else {
        await page.locator('#com').fill(comment);
        await page.locator('#password').fill(password);
        await page.getByRole('button', { name: 'Post', exact: true }).click();
        await expect(page.locator('.postMessage .mu-g')).toHaveText('green');
      }
      await expect(page.locator('.postMessage .mu-i')).toHaveText('<img src=x onerror=bad()>');
      await expect(page.locator('.postMessage .mu-b')).toHaveCSS('color', 'rgb(29, 141, 196)');
      await expect(page.locator('.postMessage img, .postMessage script')).toHaveCount(0);
      await page.reload();
      await expect(page.locator('.postMessage .mu-g')).toHaveText('green');
      const json = await (await context.request.get(`${origin}/test/thread/${op}.json`)).json();
      expect(json.posts).toHaveLength(2);
      expect(json.posts[1].com).toContain('class="mu-i"');
      await page.goto(`${origin}/test/catalog?teaser=on`);
      const teaser = page.locator(`#thread-${op} .teaser`);
      await expect(teaser).toContainText('bold');
      await expect(teaser).not.toContainText('[b]');
    } finally {
      try { if (op) expect((await context.request.post(`${origin}/test/delete`, { headers: { origin }, form: { no: op, password }, maxRedirects: 0 })).status()).toBe(303); }
      finally { await context.close(); }
    }
  });
}
