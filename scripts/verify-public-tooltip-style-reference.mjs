import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(args.length === 2 || args.length === 3 && args[2] === '--write', 'Use <core.1128.js> <desktop-css-directory> [--write]');
const manifest = JSON.parse(await readFile(new URL('docs/public-theme-reference.json', root), 'utf8'));
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const source = await readFile(args[0]); assert.equal(digest(source), manifest.client_asset.sha256);
const text = source.toString('utf8'), marker = 'Tip={node:null';
assert.equal(text.split(marker).length, 2);
const start = text.indexOf(marker), tip = text.slice(start, text.indexOf(';captchainterval=null', start));
assert.equal(Buffer.byteLength(tip), 1168);
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];
const cssNames = ['yotsubanew', 'yotsubluenew', 'futabanew', 'burichannew', 'photon', 'tomorrow'];
const cases = [];
const browser = await chromium.launch({ headless: true });
try {
  assert.equal(browser.version(), '151.0.7922.34');
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 });
  await page.route('**/*', route => route.abort());
  for (let index = 0; index < themes.length; index++) {
    const asset = manifest.assets.find(asset => asset.url.endsWith(`/${cssNames[index]}.716.css`));
    const bytes = await readFile(resolve(args[1], `${cssNames[index]}.716.css`));
    assert.equal(digest(bytes), asset.sha256);
    // Only the inspected tooltip rules enter the synthetic page. Theme assets,
    // user media, the rest of the core and extension initialization do not run.
    const rules = [...bytes.toString('utf8').matchAll(/[^{}]*?(?:tooltip|tip-top)[^{}]*\{[^}]*\}/g)].map(match => match[0]);
    assert.equal(rules.length, 5); assert.ok(!/url\(|@import/i.test(rules.join('')));
    for (const [edge, left] of [['left', 0], ['center', 640], ['right', 1250]]) {
      await page.setContent(`<html><head><style>${rules.join('\n')} body{font-family:Arial;font-size:13px;margin:0} #owned{position:absolute;top:200px;left:${left}px;width:30px;height:20px}</style></head><body><span id="owned" data-tip="Owned full &lt;label&gt; &amp; text">Short</span></body></html>`);
      await page.addScriptTag({ content: `var ${tip};` });
      const facts = await page.evaluate(() => {
        Tip.show(document.getElementById('owned'));
        const element = document.getElementById('tooltip'), style = getComputedStyle(element), arrow = getComputedStyle(element, '::before');
        const properties = ['position', 'backgroundColor', 'fontFamily', 'fontSize', 'lineHeight', 'padding', 'zIndex',
          'overflowWrap', 'whiteSpace', 'maxWidth', 'color', 'textAlign'];
        const rect = element.getBoundingClientRect();
        return { className: element.className, text: element.textContent, x: rect.x, y: rect.y, width: rect.width, height: rect.height,
          style: Object.fromEntries(properties.map(key => [key, style[key]])),
          arrow: Object.fromEntries(['borderTopColor', 'borderTopWidth', 'borderLeftWidth', 'borderRightWidth', 'bottom'].map(key => [key, arrow[key]])) };
      });
      cases.push({ theme: themes[index], edge, left, facts });
    }
  }
} finally { await browser.close(); }
const result = { scope: 'Released tooltip geometry and computed styles on a fixed synthetic target. No original-server DOM, user content, extension initialization or original full-page pixels qualified.',
  collection_date: '2026-09-30', font: 'Arial',
  browser: '151.0.7922.34', viewport: [1280, 900], density: 1, client: manifest.client_asset,
  styles: manifest.assets.map(({ url, sha256 }) => ({ url, sha256 })), cases };
const target = new URL('docs/public-tooltip-style-reference.json', root);
if (args[2] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target, 'utf8')));
console.log(`Verified ${cases.length} released tooltip style and edge-position cases without external requests.`);
