import { test, expect } from '@playwright/test';

for (const javaScriptEnabled of [false, true]) {
  test(`closed thread omits both posting forms with JavaScript ${javaScriptEnabled}`, async ({ browser }, info) => {
    const context = await browser.newContext({ javaScriptEnabled });
    try {
      const page = await context.newPage();
      for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 900 });
        await page.goto('/img/closed/1000201');
        await expect(page.locator('#t1000201')).toHaveAttribute('data-closed', 'true');
        await expect(page.locator('form.postEditor, form.postForm, #quickReply, .open-qr-link')).toHaveCount(0);
        await expect(page.locator('#m1000201')).toBeVisible();
        await expect(page.locator('#p1000201 details')).toBeVisible();
        if (javaScriptEnabled) {
          await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ keyBinds: true })));
          await page.reload();
          for (const action of [() => page.locator('#pi1000201 > .postNum').click(), () => page.keyboard.press('q')]) {
            const before = page.url();
            const warning = page.waitForEvent('dialog').then(async dialog => {
              expect(dialog.type()).toBe('alert'); expect(dialog.message()).toBe('This thread is closed'); await dialog.accept();
            });
            await action(); await warning;
            expect(page.url()).toBe(before); await expect(page.locator('#quickReply')).toHaveCount(0);
          }
        }
        const thumbnail = page.locator('#f1000201 .fileThumb img');
        await expect(thumbnail).toBeVisible();
        await expect.poll(() => thumbnail.evaluate(node => node.complete && node.naturalWidth > 0)).toBe(true);
        const path = info.outputPath(`closed-${javaScriptEnabled}-${width}.png`);
        await page.screenshot({ path }); await info.attach(`closed ${width}`, { path, contentType: 'image/png' });
        await page.goto('/img/closed-board');
        await expect(page.locator('#t1000201')).toHaveAttribute('data-closed', 'true');
        await expect(page.locator('form.postEditor')).toBeVisible();
        await expect(page.locator('form.postForm[action="/img/upload"]')).toBeVisible();
        await page.goto('/img/thread/1000201');
        await expect(page.locator('form.postEditor')).toBeVisible();
        await expect(page.locator('form.postForm[action="/img/upload"]')).toBeVisible();
      }
    } finally { await context.close(); }
  });
}
