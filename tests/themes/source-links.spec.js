import { test, expect } from '../helpers/visual-diagnostics.js';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { sourceLinkPage, sourceLinkState } from '../helpers/source-link-page.js';

const reference = JSON.parse(await readFile(new URL('../../fixtures/source-quote-styles.json', import.meta.url)));
for (const [theme, source] of Object.entries(reference.themes)) {
  test(`${theme} retains source quote and deadlink presentation`, async ({ page, context }, info) => {
    expect(createHash('sha256').update(source.excerpt).digest('hex')).toBe(source.excerpt_sha256);
    const oracle = await context.newPage();
    await oracle.route('**/*', route => route.abort());
    await oracle.setContent(`<!doctype html><style>${source.excerpt}</style><a class="quotelink">&gt;&gt;123</a><span class="deadlink">&gt;&gt;999</span>`);
    const styles = await oracle.evaluate(() => Object.fromEntries(['quotelink', 'deadlink'].map(name => {
      const style = getComputedStyle(document.querySelector(`.${name}`));
      return [name, { color: style.color, decoration: style.textDecorationLine }];
    })));
    // Keep the independently styled full original page open for paired review.
    await oracle.setContent(sourceLinkPage(theme));
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await oracle.setViewportSize({ width, height: 900 });
      await page.goto('/link-visual/');
      expect(await sourceLinkState(page)).toEqual(await sourceLinkState(oracle));
      const originalPath = info.outputPath(`${theme}-${width}-original-source-links.png`);
      await oracle.screenshot({ path: originalPath, fullPage: true });
      await info.attach(`${theme} ${width} original source page`, { path: originalPath, contentType: 'image/png' });
      const message = page.locator('#m123');
      for (const [label, href] of [['>>123', '#p123'], ['>>000123', '#p000123'],
        ['>>>/g/+123', '/g/thread/100#p+123'], ['>>>/g/123e0', '/g/thread/100#p123e0']]) {
        const link = message.getByRole('link', { name: label, exact: true });
        await expect(link).toHaveAttribute('href', href);
        await expect(link).toHaveCSS('color', styles.quotelink.color);
      }
      const dead = message.locator('.deadlink');
      await expect(dead).toHaveText(['>>999', '>>>/g/-123', '>>>/g/1e309']);
      for (const node of await dead.all()) {
        await expect(node).toHaveCSS('color', styles.deadlink.color);
        await expect(node).toHaveCSS('text-decoration-line', styles.deadlink.decoration);
        await expect(node).not.toHaveAttribute('href');
      }
      await expect(message.locator('a[href*="example.test"]')).toHaveCount(0);
      await expect(message.getByRole('link', { name: 'https://www.4chan.org/faq', exact: true })).toHaveAttribute('href', 'https://www.4chan.org/faq');
      await expect(message).toContainText('entityneedle & <tag> "quoted" \'apostrophe\' &amp; &#39;');
      await expect(message.locator('tag, script')).toHaveCount(0);
      const path = info.outputPath(`${theme}-${width}-source-links.png`);
      await page.screenshot({ path, fullPage: true });
      await info.attach(`${theme} ${width} source link page`, { path, contentType: 'image/png' });
    }
    await oracle.close();
  });
}
