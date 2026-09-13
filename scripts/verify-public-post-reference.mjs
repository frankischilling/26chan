// Reproduce recorded facts with locally retained, hash-checked public CSS.
// No stylesheet or user content is downloaded by this command.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

assert.equal(process.argv.length, 3, 'usage: node scripts/verify-public-post-reference.mjs <pinned-css-directory>');
const reference = JSON.parse(await readFile(new URL('../docs/public-post-layout-reference.json', import.meta.url), 'utf8'));
const manifest = JSON.parse(await readFile(new URL('../docs/public-theme-reference.json', import.meta.url), 'utf8'));
const html = await readFile(new URL('../tests/themes/reference-post.html', import.meta.url), 'utf8');
const browser = await chromium.launch();
try {
  assert.equal(browser.version(), reference.environment.chromium);
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 });
  await page.route('**/*', route => route.abort());
  for (const [theme, expected] of Object.entries(reference.themes)) {
    assert.match(expected.source, /^[a-z]+\.716\.css$/);
    const source = await readFile(resolve(process.argv[2], expected.source));
    const pin = manifest.assets.find(asset => new URL(asset.url).pathname.endsWith('/' + expected.source));
    assert.ok(pin);
    assert.equal(source.length, pin.bytes);
    assert.equal(createHash('sha256').update(source).digest('hex'), pin.sha256);
    await page.setContent(html);
    await page.addStyleTag({ content: source.toString('utf8') });
    const actual = await page.evaluate(() => {
      const style = selector => getComputedStyle(document.querySelector(selector));
      const op = style('.op'), reply = style('.reply'), comment = style('.postMessage');
      const arrows = style('.sideArrows'), thumbnail = style('.fileThumb');
      return {
        common: { op_display: op.display, op_padding: op.padding, reply_display: reply.display,
          reply_padding: reply.padding, first_reply_margin: reply.margin, line_height: comment.lineHeight,
          comment_horizontal_margin: comment.marginLeft, thumbnail_float: thumbnail.cssFloat,
          thumbnail_margin: thumbnail.margin, arrows_float: arrows.cssFloat, arrows_margin: arrows.margin },
        comment_margin: comment.margin, border_width: reply.borderWidth,
        border_right: reply.borderRightColor, arrows: arrows.color,
      };
    });
    assert.deepEqual(actual.common, reference.common, theme);
    for (const key of ['comment_margin', 'border_width', 'border_right', 'arrows']) {
      assert.equal(actual[key], expected[key], `${theme}: ${key}`);
    }
    await page.evaluate(() => { location.hash = 'p2'; });
    assert.equal(await page.locator('.reply').evaluate(node => getComputedStyle(node).borderRightColor), expected.target_border_right, theme);
    await page.evaluate(() => { location.hash = ''; });
    console.log(`PASS ${theme}: pinned public stylesheet reproduces recorded post properties`);
  }
} finally {
  await browser.close();
}
