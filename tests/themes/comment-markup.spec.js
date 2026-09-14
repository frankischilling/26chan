import { test, expect } from '@playwright/test';

for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  test(`${theme} renders stamped multiline spoilers, code and SJIS without interpreting text`, async ({ page, context }, info) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto('/markup/');
      const spoiler = page.locator('#m1000001 s').first();
      await expect(spoiler).toHaveText('hiddensecond');
      await expect(spoiler.locator('br')).toHaveCount(1);
      await page.mouse.move(0, 0);
      await expect(spoiler).toHaveCSS('color', 'rgb(0, 0, 0)');
      await expect(spoiler).toHaveCSS('background-color', 'rgb(0, 0, 0)');
      await spoiler.hover();
      await expect(spoiler).toHaveCSS('color', 'rgb(255, 255, 255)');
      const code = page.locator('#m1000002 pre.prettyprint');
      await expect(code).toHaveText('first  linesecond <script>line</script>');
      await expect(code.locator('br')).toHaveCount(1);
      await expect(code).toHaveCSS('padding', '5px');
      await expect(code).toHaveCSS('margin', '0px');
      await expect(code).toHaveCSS('display', 'inline-block');
      await expect(code).toHaveCSS('max-height', '400px');
      await expect(code).toHaveCSS('max-width', width === 390 ? '300px' : '600px');
      await expect(code).toHaveCSS('background-color', width === 390 ? 'rgb(255, 255, 255)' :
        theme === 'tomorrow' ? 'rgba(255, 255, 255, 0.1)' : theme === 'photon' ? 'rgba(150, 150, 150, 0.2)' : 'rgb(255, 255, 255)');
      const art = page.locator('#m1000003 .sjis');
      await expect(art).toHaveText('a  b c');
      await expect(art.locator('br')).toHaveCount(1);
      await expect(art).toHaveCSS('font-size', '16px');
      await expect(art).toHaveCSS('line-height', '17px');
      await expect(art).toHaveCSS('white-space', 'pre');
      const op = page.locator('#m1000004');
      await expect(op.locator('.mu-s').first()).toHaveCSS('font-weight', '700');
      await expect(op.locator('.mu-i')).toHaveCSS('font-style', 'italic');
      await expect(op.locator('.mu-r')).toHaveCSS('color', 'rgb(196, 30, 58)');
      await expect(op.locator('.mu-g')).toHaveCSS('color', 'rgb(0, 165, 80)');
      await expect(op.locator('.mu-b')).toHaveCSS('color', 'rgb(29, 141, 196)');
      await expect(op.locator('.mu-s').last()).toHaveText('<script>text stays text</script>');
      await expect(page.locator('.postMessage img, .postMessage script')).toHaveCount(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      const path = info.outputPath(`${theme}-${width}-markup.png`);
      await page.locator('.board').screenshot({ path });
      await info.attach(`${theme} ${width} source markup`, { path, contentType: 'image/png' });
    }
  });
}
