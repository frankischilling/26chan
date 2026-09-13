import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-post-layout-reference.json', import.meta.url), 'utf8'));

for (const [theme, values] of Object.entries(reference.themes)) {
  test(`referenced desktop post layout in ${theme}`, async ({ page, context }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await page.goto('/demo/');
    const common = reference.common;
    await expect(page.locator('.op')).toHaveCSS('display', common.op_display);
    await expect(page.locator('.op')).toHaveCSS('padding', common.op_padding);
    await expect(page.locator('.reply')).toHaveCSS('display', common.reply_display);
    await expect(page.locator('.reply')).toHaveCSS('padding', common.reply_padding);
    await expect(page.locator('.reply')).toHaveCSS('margin', common.first_reply_margin);
    await expect(page.locator('.reply')).toHaveCSS('border-width', values.border_width);
    await expect(page.locator('.reply')).toHaveCSS('border-right-color', values.border_right);
    await expect(page.locator('.postInfo').first()).toHaveCSS('line-height', common.line_height);
    await expect(page.locator('.postMessage').first()).toHaveCSS('margin', values.comment_margin);
    await expect(page.locator('.postMessage').first()).toHaveCSS('line-height', common.line_height);
    await expect(page.locator('.sideArrows')).toHaveText('>>');
    await expect(page.locator('.sideArrows')).toHaveAttribute('aria-hidden', 'true');
    await expect(page.locator('.sideArrows')).toHaveCSS('float', common.arrows_float);
    await expect(page.locator('.sideArrows')).toHaveCSS('margin', common.arrows_margin);
    await expect(page.locator('.sideArrows')).toHaveCSS('color', values.arrows);
    await page.goto('/demo/#p1000002');
    await expect(page.locator('.reply')).toHaveCSS('border-right-color', values.target_border_right);

    await page.goto('/img/thread/1000201');
    const thumbnail = page.locator('.fileThumb').first();
    await expect(thumbnail).toHaveCSS('float', common.thumbnail_float);
    await expect(thumbnail).toHaveCSS('margin', common.thumbnail_margin);
    expect(await page.locator('.op').evaluate(node => {
      const file = node.querySelector('.file');
      const info = node.querySelector('.postInfo');
      return Boolean(file.compareDocumentPosition(info) & Node.DOCUMENT_POSITION_FOLLOWING);
    })).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  });
}
