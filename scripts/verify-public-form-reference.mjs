// Local, hash-pinned public CSS on synthetic fields; no original client execution.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

assert.equal(process.argv.length, 3, 'usage: node scripts/verify-public-form-reference.mjs <pinned-css-directory>');
const reference = JSON.parse(await readFile(new URL('../docs/public-form-reference.json', import.meta.url), 'utf8'));
const manifest = JSON.parse(await readFile(new URL('../docs/public-theme-reference.json', import.meta.url), 'utf8'));
const html = await readFile(new URL('../tests/themes/reference-form.html', import.meta.url), 'utf8');
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
    assert.equal(await page.evaluate(() => document.compatMode), 'CSS1Compat');
    const actual = await page.evaluate(() => {
      const style = selector => getComputedStyle(document.querySelector(selector));
      const table = style('table'), label = style('td:first-child');
      const field = style('input[name=name]'), comment = style('textarea');
      return {
        common: { table_width: table.width, table_spacing: table.borderSpacing,
          field_width: field.width, textarea_width: comment.width, field_size: field.fontSize,
          box_sizing: field.boxSizing, label_alignment: label.verticalAlign },
        label_padding: label.padding, label_border: label.borderWidth, label_size: label.fontSize,
        field_padding: field.padding, field_margin: field.margin,
        textarea_padding: comment.padding, textarea_margin: comment.margin,
        textarea_height: comment.height, textarea_font: comment.fontFamily,
      };
    });
    assert.deepEqual(actual.common, reference.common, theme);
    for (const key of Object.keys(expected).filter(key => key !== 'source')) {
      assert.equal(actual[key], expected[key], `${theme}: ${key}`);
    }
    console.log(`PASS ${theme}: pinned public stylesheet reproduces recorded form properties`);
  }
} finally {
  await browser.close();
}
