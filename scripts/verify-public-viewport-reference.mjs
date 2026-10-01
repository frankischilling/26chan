// Whole pinned public clients on owned short pages; only viewport paint below
// body bounds is compared. This does not replay full original page startup.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';
import { screenshotPixel } from './viewport-pixels.mjs';

const args = process.argv.slice(2), root = new URL('../', import.meta.url);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <reference-directory> [--write]');
const boardManifest = JSON.parse(await readFile(new URL('docs/public-theme-reference.json', root)));
const catalogManifest = JSON.parse(await readFile(new URL('docs/public-catalog-ui-reference.json', root)));
const navigation = JSON.parse(await readFile(new URL('docs/public-watcher-navigation-reference.json', root)));
const extensionPin = JSON.parse(await readFile(new URL('docs/public-settings-transfer-reference.json', root)));
const pins = [...boardManifest.assets, ...catalogManifest.styles, ...navigation.stylesheets,
  ...boardManifest.background_assets, boardManifest.client_asset, catalogManifest.client];
const sources = [];
async function source(name, pin = pins.find(row => String(row.path ?? new URL(row.url).pathname).split('/').at(-1) === name), path = resolve(args[0], name)) {
  assert.ok(pin, `Missing pin: ${name}`);
  const bytes = await readFile(path);
  assert.equal(bytes.length, pin.bytes); assert.equal(createHash('sha256').update(bytes).digest('hex'), pin.sha256);
  sources.push({ name, url: pin.url ?? pin.source ?? null, bytes: pin.bytes, sha256: pin.sha256 });
  return { bytes, text: bytes.toString('utf8') };
}
const core = await source('core.min.1128.js'), extension = await source('extension.1191.js', extensionPin);
const catalogClient = await source('catalog.min.1025.js');
const mobile = { true: await source('yotsubluemobile.716.css'), false: await source('yotsubamobile.716.css') };
const catalogMobile = await source('catalog_mobile.705.css');
const images = new Map();
for (const pin of boardManifest.background_assets) {
  const name = new URL(pin.url).pathname.split('/').at(-1);
  images.set(pin.url, await source(name, pin, new URL(`apps/public/static/themes/${name}`, root)));
}
const names = { yotsuba: ['yotsubanew', 'yotsuba_new'], 'yotsuba-b': ['yotsubluenew', 'yotsuba_b_new'],
  futaba: ['futabanew', 'futaba_new'], burichan: ['burichannew', 'burichan_new'], photon: ['photon', 'photon'], tomorrow: ['tomorrow', 'tomorrow'] };
const styles = [];
for (const [theme, [board, catalog]] of Object.entries(names)) {
  styles.push({ theme, mode: 'index', ...(await source(`${board}.716.css`)) });
  styles.push({ theme, mode: 'catalog', themeClass: catalog, ...(await source(`catalog_${catalog}.705.css`)) });
}
const origin = 'https://boards.reference.invalid', cases = [];
const height = 1200;
const browser = await chromium.launch();
try {
  assert.equal(browser.version(), '151.0.7922.34');
  for (const style of styles) for (const worksafe of [true, false]) for (const width of [390, 480, 481, 1280]) for (const scale of [1, 2]) {
    const states = [{ neverMobile: false, dark: false }];
    if (width <= 480) {
      states.push({ neverMobile: true, dark: false });
      if (style.mode === 'index') states.push({ neverMobile: false, dark: true });
    }
    for (const state of states) {
      const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: scale });
      const errors = [], denied = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.route('**/*', route => {
        const request = route.request();
        if (images.has(request.url())) return route.fulfill({ body: images.get(request.url()).bytes, contentType: 'image/png' });
        if (request.isNavigationRequest() && request.url() === `${origin}/demo/`) {
          const mobileCss = state.neverMobile ? '' : style.mode === 'catalog' ? catalogMobile.text : mobile[worksafe].text;
          const nav = '<div id="boardNavDesktop" class="desktop"><span class="boardList">[ <a href="/demo/" title="Owned demo">demo</a> ]</span><span id="navtopright"><a id="settingsWindowLink" href="#">Settings</a></span></div>';
          return route.fulfill({ contentType: 'text/html', body: `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><style>${style.text}\n${mobileCss}</style></head><body class="${worksafe ? 'ws' : 'nws'} ${style.mode === 'catalog' ? `is_catalog ${style.themeClass}` : ''}">${style.mode === 'catalog' ? `<div id="topnav" class="boardnav">${nav}</div>` : nav}<div id="boardNavMobile" class="mobile"><div class="boardSelect"><strong>Board</strong> <select id="boardSelectMobile"></select></div><div class="pageJump"><a href="#bottom">▼</a><a id="settingsWindowLinkMobile" href="#">Settings</a><a href="/">Home</a></div></div><div class="boardBanner"><div id="bannerCnt" class="title desktop"></div><div class="boardTitle">/demo/ - Owned demo</div><div class="boardSubtitle">Owned description</div></div><main style="height:200px">Owned empty page</main><div id="absbot" class="absBotText"><div id="footer-links"><a href="/about">About</a></div></div><div id="bottom"></div></body></html>` });
        }
        denied.push(new URL(request.url()).origin + new URL(request.url()).pathname); return route.abort();
      });
      await page.goto(`${origin}/demo/`);
      await page.evaluate(neverMobile => { window.FC = {}; window.style_group = 'ws_style'; localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })); if (neverMobile) localStorage.setItem('4chan_never_show_mobile', 'true'); }, state.neverMobile);
      await page.addScriptTag({ content: core.text });
      await page.evaluate(() => { buildMobileNav(); cloneTopNav(); });
      await page.addScriptTag({ content: extension.text });
      await page.evaluate(dark => { window.$L = { d: () => 'reference.invalid' }; Main.addCSS(); document.body.classList.toggle('m-dark', dark); }, state.dark);
      const values = await page.evaluate(() => {
        const computed = getComputedStyle(document.documentElement);
        return { root: Object.fromEntries(['backgroundColor', 'backgroundImage', 'backgroundRepeat', 'backgroundPosition', 'backgroundSize'].map(key => [key, computed[key]])), bodyBottom: document.body.getBoundingClientRect().bottom };
      });
      const point = { x: Math.floor(width / 2), y: height - 1 };
      assert.ok(values.bodyBottom < point.y - 1, 'Sample must lie below the short owned body');
      const pixel = screenshotPixel(await page.screenshot(), point, scale);
      assert.equal(pixel[3], 255); assert.deepEqual(errors, []); assert.ok(denied.length <= 16);
      cases.push({ theme: style.theme, mode: style.mode, worksafe, width, scale, ...state, root: values.root, point, pixel });
      await page.close();
    }
  }
} finally { await browser.close(); }
const result = { collection_date: '2026-10-01', browser: '151.0.7922.34', height, sources,
  scope: 'Whole pinned Core v1128 and extension v1191 loaded after DOMContentLoaded on owned short board/catalog markup. Original buildMobileNav, cloneTopNav and Main.addCSS called; direct m-dark class fact on mobile index pages, and the released literal mobile opt-out removes only the mobile stylesheet. Catalog theme class is supplied from pinned v1025. Only admitted fixed gradient PNGs can load; all other nonfixture requests abort. Root background properties and one opaque viewport pixel below the body are recorded at four responsive widths and two densities. This does not qualify full page pixels, body height, content placement, catalog dark mode or original startup.', cases };
const target = new URL('docs/public-viewport-reference.json', root);
if (args[1] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target)));
console.log(`Public viewport paint: ${cases.length} owned source cases match.`);
