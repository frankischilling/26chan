import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <pinned-css-directory> [--write]');
const json = async path => JSON.parse(await readFile(new URL(path, root)));
const desktop = await json('docs/public-theme-reference.json'), mobile = await json('docs/public-watcher-navigation-reference.json');
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];
const styles = ['yotsubanew', 'yotsubluenew', 'futabanew', 'burichannew', 'photon', 'tomorrow'];
const css = new Map(), pins = [];
for (const name of [...styles, 'yotsubamobile', 'yotsubluemobile']) {
  const pin = [...desktop.assets, ...mobile.stylesheets].find(row => row.url.endsWith(`/${name}.716.css`));
  assert.ok(pin);
  const data = await readFile(resolve(args[0], `${name}.716.css`));
  assert.equal(createHash('sha256').update(data).digest('hex'), pin.sha256);
  css.set(name, data.toString('utf8')); pins.push({ url: pin.url, sha256: pin.sha256 });
}
const browser = await chromium.launch({ headless: true });
const cases = [];
try {
  assert.equal(browser.version(), '151.0.7922.34');
  const page = await browser.newPage({ deviceScaleFactor: 1 });
  await page.route('**/*', route => route.abort());
  for (const family of ['yotsubamobile', 'yotsubluemobile']) for (const viewport of [[1280, 900], [390, 844]]) {
    await page.setViewportSize({ width: viewport[0], height: viewport[1] });
    for (const [i, style] of styles.entries()) {
      await page.setContent(`<!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1"><style>${css.get(style)}\n${css.get(family)}</style></head><body><div class="board"><div class="thread"><div class="post op"><div class="file"><div class="fileText">File: <a href="https://media.invalid/demo/1001001.png">Owned.png</a> (2 KB, 600x360)</div><a class="fileThumb" href="https://media.invalid/demo/1001001.png"><img width="250" height="150" style="width:250px;height:150px" alt="Owned"><div class="mFileInfo mobile">2 KB PNG</div></a></div></div></div></div></body></html>`);
      const values = await page.evaluate(() => {
        const facts = {};
        for (const [key, selector, properties] of [
          ['header', '.fileText', ['display', 'whiteSpace', 'maxWidth']],
          ['thumb', '.fileThumb', ['float', 'margin', 'textDecorationLine']],
          ['image', '.fileThumb img', ['float', 'maxWidth', 'maxHeight', 'objectFit', 'width', 'height']],
          ['caption', '.mFileInfo', ['display', 'paddingTop', 'textAlign', 'color', 'fontSize', 'textDecorationLine']],
        ]) {
          const node = document.querySelector(selector), computed = getComputedStyle(node);
          facts[key] = Object.fromEntries(properties.map(property => [property, computed[property]]));
        }
        return facts;
      });
      cases.push({ family, viewport, theme: themes[i], values });
    }
  }
} finally { await browser.close(); }
const result = { collection_date: '2026-09-30', browser: '151.0.7922.34', density: 1, styles: pins,
  scope: 'Pinned public stylesheets on synthetic released file DOM with a 250x150 thumbnail. Requests are denied. Properties qualify file layout, not original-page pixels or spoiler/deleted-file behavior.', cases };
const target = new URL('docs/public-file-style-reference.json', root);
if (args[1] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target, 'utf8')));
console.log(`Verified ${cases.length} independent desktop/mobile file style cases.`);
