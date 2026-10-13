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
    for (const worksafe of [true, false]) {
      const family = worksafe ? 'ws' : 'nws';
      await oracle.setContent(sourceLinkPage(theme, worksafe));
      await context.addCookies([{ name: `board-theme-${family}`, value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
      for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 900 });
        await oracle.setViewportSize({ width, height: 900 });
        await page.mouse.move(0, 0);
        await oracle.mouse.move(0, 0);
        await page.goto(worksafe ? '/link-visual/' : '/link-visual-nws/');
        for (const [name, target] of [['original', oracle], ['replacement', page]]) {
          const path = info.outputPath(`${theme}-${family}-${width}-${name}-source-links.png`);
          await target.screenshot({ path, fullPage: true });
          await info.attach(`${theme} ${family} ${width} ${name} source link page`, { path, contentType: 'image/png' });
        }
        expect(await sourceLinkState(page)).toEqual(await sourceLinkState(oracle));
        const message = page.locator('#m123');
        for (const [label, href] of [['>>123', '#p123'], ['>>000123', '#p000123'],
          ['>>>/g/+123', '/g/thread/100#p+123'], ['>>>/g/123e0', '/g/thread/100#p123e0']]) {
          await expect(message.getByRole('link', { name: label, exact: true })).toHaveAttribute('href', href);
        }
        const dead = message.locator('.deadlink');
        await expect(dead).toHaveText(['>>999', '>>>/g/-123', '>>>/g/1e309']);
        for (const node of await dead.all()) {
          await expect(node).toHaveCSS('text-decoration-line', 'line-through');
          await expect(node).not.toHaveAttribute('href');
        }
        await expect(message.locator('a[href*="example.test"]')).toHaveCount(0);
        await expect(message).toContainText('entityneedle & <tag> "quoted" \'apostrophe\' &amp; &#39;');
        await expect(message.locator('tag, script')).toHaveCount(0);
        for (const label of ['>>123', 'https://www.4chan.org/faq']) {
          await message.getByRole('link', { name: label, exact: true }).hover();
          await oracle.locator('#m123').getByRole('link', { name: label, exact: true }).hover();
          expect(await sourceLinkState(page)).toEqual(await sourceLinkState(oracle));
        }
        await page.mouse.move(0, 0);
        await oracle.mouse.move(0, 0);
        if (width === 390) {
          // Only the two pinned dark-link rules are added to the original page;
          // this probe does not claim an entire extension-enabled page render.
          await page.evaluate(() => document.body.classList.add('m-dark'));
          await oracle.evaluate(() => document.body.classList.add('m-dark'));
          expect(await sourceLinkState(page)).toEqual(await sourceLinkState(oracle));
          for (const label of ['>>123', 'https://www.4chan.org/faq']) {
            await message.getByRole('link', { name: label, exact: true }).hover();
            await oracle.locator('#m123').getByRole('link', { name: label, exact: true }).hover();
            expect(await sourceLinkState(page)).toEqual(await sourceLinkState(oracle));
          }
          await page.mouse.move(0, 0);
          await oracle.mouse.move(0, 0);
          await page.evaluate(() => document.body.classList.remove('m-dark'));
          await oracle.evaluate(() => document.body.classList.remove('m-dark'));
          // The existing Never use mobile layout preference also gates this cascade.
          await page.evaluate(() => document.body.setAttribute('data-native-never-mobile', 'true'));
          await oracle.setViewportSize({ width: 1280, height: 900 });
          expect(await sourceLinkState(page)).toEqual(await sourceLinkState(oracle));
          await page.evaluate(() => document.body.removeAttribute('data-native-never-mobile'));
        }
      }
      // Source mobile CSS changes at 480px, not at the wider layout breakpoint.
      for (const width of [480, 481]) {
        await page.setViewportSize({ width, height: 900 });
        await oracle.setViewportSize({ width, height: 900 });
        expect(await sourceLinkState(page)).toEqual(await sourceLinkState(oracle));
      }
    }
    await oracle.close();
  });
}
