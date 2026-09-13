import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import { chromium } from '@playwright/test';

assert.equal(process.argv.length, 3, 'usage: node scripts/verify-public-catalog-reference.mjs <pinned-asset-directory>');
const reference = JSON.parse(await readFile(new URL('../docs/public-catalog-reference.json', import.meta.url), 'utf8'));
const html = await readFile(new URL('../tests/themes/reference-catalog.html', import.meta.url), 'utf8');
const sources = new Map();
for (const asset of reference.assets) {
  const bytes = await readFile(resolve(process.argv[2], basename(asset.path)));
  assert.equal(bytes.length, asset.bytes, asset.path);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), asset.sha256, asset.path);
  sources.set(basename(asset.path), bytes.toString('utf8'));
}
const browser = await chromium.launch();
try {
  assert.equal(browser.version(), reference.environment.chromium);
  const page = await browser.newPage({ deviceScaleFactor: 1 });
  await page.route('**/*', route => route.abort());
  for (const [theme, source] of Object.entries(reference.themes)) {
    for (const [width, height] of reference.environment.viewports) {
      await page.setViewportSize({ width, height });
      await page.setContent(html);
      await page.locator('img').evaluate(img => img.decode());
      for (const name of [`catalog_${source}.705.css`, 'catalog_mobile.705.css']) {
        await page.addStyleTag({ content: sources.get(name) });
      }
      const actual = await page.evaluate(keys => {
        const style = selector => getComputedStyle(document.querySelector(selector));
        const card = style('.thread');
        return { mode: document.compatMode, common: Object.fromEntries(keys.map(key => [key, card[key]])),
          width: card.width, meta: [style('.meta').fontSize, style('.meta').lineHeight, style('.meta').margin],
          teaser: style('.teaser').padding, thumb: [style('.thumb').display, style('.thumb').minWidth, style('.thumb').minHeight] };
      }, Object.keys(reference.common));
      assert.equal(actual.mode, 'CSS1Compat');
      assert.deepEqual(actual.common, reference.common);
      assert.equal(actual.width, width <= 480 ? '155px' : '180px');
      assert.deepEqual(actual.meta, ['11px', '8px', '2px 0px 1px']);
      assert.equal(actual.teaser, '0px 15px');
      assert.deepEqual(actual.thumb, ['inline', '50px', '50px']);
      const tiny = await page.locator('img').evaluate(img => {
        img.width = 48; img.height = 32;
        const box = img.getBoundingClientRect();
        return [box.width, box.height];
      });
      assert.deepEqual(tiny, [50, 50], 'public CSS independently floors both thumbnail axes');
      console.log(`PASS ${theme} ${width}: pinned catalog card properties`);
    }
  }
} finally { await browser.close(); }
