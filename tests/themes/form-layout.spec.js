import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
const reference = JSON.parse(await readFile(new URL('../../docs/public-form-reference.json', import.meta.url), 'utf8'));
for (const [theme, value] of Object.entries(reference.themes)) {
  test(`referenced desktop posting fields in ${theme}`, async ({ page, context }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await page.goto('/demo/');
    const common = reference.common;
    const form = page.locator('#postForm');
    await expect(form).toHaveCSS('width', common.table_width);
    await expect(form).toHaveCSS('border-spacing', common.table_spacing);
    const label = form.locator('td').first();
    await expect(label).toHaveCSS('padding', value.label_padding);
    await expect(label).toHaveCSS('border-width', value.label_border);
    await expect(label).toHaveCSS('font-size', value.label_size);
    await expect(label).toHaveCSS('vertical-align', common.label_alignment);
    const name = page.getByLabel('Name', { exact: true });
    await expect(name).toHaveCSS('width', common.field_width);
    await expect(name).toHaveCSS('padding', value.field_padding);
    await expect(name).toHaveCSS('margin', value.field_margin);
    await expect(name).toHaveCSS('font-size', common.field_size);
    await expect(name).toHaveCSS('box-sizing', common.box_sizing);
    const comment = page.getByLabel('Comment', { exact: true });
    await expect(comment).toHaveCSS('width', common.textarea_width);
    await expect(comment).toHaveCSS('height', value.textarea_height);
    await expect(comment).toHaveCSS('padding', value.textarea_padding);
    await expect(comment).toHaveCSS('margin', value.textarea_margin);
    await expect(comment).toHaveCSS('font-family', value.textarea_font);
    await expect(comment).toHaveCSS('box-sizing', common.box_sizing);
    await expect(comment).toHaveAttribute('rows', '4');
    await name.fill('Synthetic author');
    await comment.fill('Synthetic draft');
    await expect(page.getByRole('button', { name: 'Post', exact: true })).toBeEnabled();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    for (const route of ['/demo/', '/img/thread/1000201']) {
      await page.setViewportSize({ width: 390, height: 844 });
      await page.goto(route);
      await expect(page.locator('table#postForm')).toHaveAttribute('role', 'presentation');
      await expect(page.getByLabel('Comment', { exact: true })).toHaveCSS('font-size', '16px');
      await page.getByLabel('Name', { exact: true }).fill('Synthetic author');
      await page.getByLabel('Comment', { exact: true }).fill('Synthetic mobile draft');
      await expect(page.getByLabel('Comment', { exact: true })).toHaveCSS('outline-width', '2px');
      expect(await page.locator('form.postEditor').evaluate(node => node.checkValidity())).toBe(false);
      await page.locator('form.postEditor').getByLabel('Deletion password', { exact: true }).fill('synthetic-form-password');
      expect(await page.locator('form.postEditor').evaluate(node => node.checkValidity())).toBe(true);
      await expect(page.locator('form.postEditor button[type=submit]')).toHaveCount(1);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    }
  });
}
