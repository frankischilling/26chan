import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import { chromium } from '@playwright/test';

assert.equal(process.argv.length, 3, 'usage: node scripts/verify-public-catalog-assets.mjs <pinned-catalog-css-directory>');
const assets = JSON.parse(await readFile(new URL('../docs/public-catalog-assets.json', import.meta.url), 'utf8'));
const reference = JSON.parse(await readFile(new URL('../docs/public-catalog-reference.json', import.meta.url), 'utf8'));
let html = await readFile(new URL('../tests/themes/reference-catalog-assets.html', import.meta.url), 'utf8');
const substitutions = [];
for (const asset of assets.assets) {
  const bytes = await readFile(new URL(`../apps/public/static/catalog/${asset.name}`, import.meta.url));
  assert.equal(bytes.length, asset.bytes);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), asset.sha256, asset.name);
  const embedded = `data:${asset.mime};base64,${bytes.toString('base64')}`;
  html = html.replaceAll(`${assets.local_base}${asset.name}`, embedded);
  substitutions.push([`//s.4cdn.org/image/${asset.name}`, embedded]);
}
const sources = new Map();
for (const asset of reference.assets) {
  const bytes = await readFile(resolve(process.argv[2], basename(asset.path)));
  assert.equal(bytes.length, asset.bytes);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), asset.sha256, asset.path);
  let css = bytes.toString('utf8');
  for (const [remote, local] of substitutions) css = css.replaceAll(remote, local);
  sources.set(basename(asset.path), css);
}
const browser = await chromium.launch();
try {
  assert.equal(browser.version(), assets.environment.chromium);
  for (const scale of assets.environment.device_scale_factors) {
    const page = await browser.newPage({ deviceScaleFactor: scale });
    await page.route('**/*', route => route.abort());
    for (const [theme, source] of Object.entries(reference.themes)) {
      for (const [width, height] of assets.environment.viewports) {
        await page.setViewportSize({ width, height });
        await page.setContent(html);
        for (const name of [`catalog_${source}.705.css`, 'catalog_mobile.705.css']) await page.addStyleTag({ content: sources.get(name) });
        await page.locator('img').evaluateAll(imgs => Promise.all(imgs.map(img => img.decode())));
        for (const mode of assets.environment.modes) {
          await page.locator('#threads').evaluate((node, mode) => { node.className = mode; }, mode);
          for (const [selector, expected] of Object.entries(assets.styles)) {
            const actual = await page.locator(selector).first().evaluate((node, keys) => {
              const box = node.getBoundingClientRect(); const style = getComputedStyle(node);
              return { box: [box.width, box.height], css: Object.fromEntries(keys.map(key => [key,style[key]])) };
            }, Object.keys(expected.css));
            assert.deepEqual(actual, expected, `${theme} ${width} ${scale} ${mode} ${selector}`);
          }
          for (const name of ['sticky','closed']) {
            const actual = await page.locator(`.${name}Icon`).evaluate(node => {
              const style = getComputedStyle(node); return [style.backgroundImage, style.backgroundSize];
            });
            const assetName = `${name}${scale === 2 ? '@2x' : ''}.gif`;
            const embedded = substitutions.find(([remote]) => remote.endsWith(`/${assetName}`))[1];
            assert.deepEqual(actual, [`url("${embedded}")`, scale === 2 ? '100%' : 'auto']);
          }
        }
        console.log(`PASS ${theme} ${width} scale ${scale}: all catalog asset modes`);
      }
    }
    await page.close();
  }
} finally { await browser.close(); }
