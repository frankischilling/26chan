// Whole released Core and extension; owned mobile navigation only.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

const args = process.argv.slice(2), root = new URL('../', import.meta.url);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <reference-directory> [--write]');
const themes = JSON.parse(await readFile(new URL('docs/public-theme-reference.json', root)));
const mobile = JSON.parse(await readFile(new URL('docs/public-watcher-navigation-reference.json', root)));
const extensionPin = JSON.parse(await readFile(new URL('docs/public-settings-transfer-reference.json', root)));
const pins = [...themes.assets, ...mobile.stylesheets, themes.client_asset];
const sources = [];
async function source(name, pin = pins.find(pin => String(pin.path ?? new URL(pin.url).pathname).split('/').pop() === name)) {
  assert.ok(pin, `Missing pin: ${name}`);
  const bytes = await readFile(resolve(args[0], name));
  assert.equal(bytes.length, pin.bytes); assert.equal(createHash('sha256').update(bytes).digest('hex'), pin.sha256);
  sources.push({ name, bytes: pin.bytes, sha256: pin.sha256 }); return bytes.toString('utf8');
}
const core = await source('core.min.1128.js'), extension = await source('extension.1191.js', extensionPin);
const mobileStyles = { true: await source('yotsubluemobile.716.css'), false: await source('yotsubamobile.716.css') };
const styles = [];
for (const [theme, name] of [['yotsuba', 'yotsubanew'], ['yotsuba-b', 'yotsubluenew'], ['futaba', 'futabanew'],
  ['burichan', 'burichannew'], ['photon', 'photon'], ['tomorrow', 'tomorrow']]) styles.push({ theme, css: await source(`${name}.716.css`) });
const selectors = { page: 'body', banner: '.boardBanner', desktop: '#boardNavDesktop',
  desktopLink: '#boardNavDesktop .boardList a', mobile: '#boardNavMobile', mobileSelect: '#boardSelectMobile',
  mobileLink: '#settingsWindowLinkMobile', title: '.boardTitle', subtitle: '.boardSubtitle', footer: '#boardNavDesktopFoot',
  footerLink: '#navbotright a', footerLinks: '#footer-links', footerText: '#absbot' };
const properties = ['color', 'backgroundColor', 'backgroundImage', 'borderBottomColor', 'borderBottomStyle', 'borderBottomWidth'];
const origin = 'https://boards.reference.invalid', cases = [];
const browser = await chromium.launch();
try {
  assert.equal(browser.version(), '151.0.7922.34');
  for (const style of styles) for (const worksafe of [true, false]) for (const width of [390, 480]) for (const scale of [1, 2]) {
    const page = await browser.newPage({ viewport: { width, height: 900 }, deviceScaleFactor: scale });
    const errors = [], denied = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/*', route => {
      if (route.request().isNavigationRequest() && route.request().url() === `${origin}/demo/`) {
        const list = ['zed', 'demo', 'f'].map(board => `<span${board === 'zed' ? ' class="nwsb"' : ''}><a href="/${board}/" title="Owned ${board}">${board}</a></span>`).join(' / ');
        return route.fulfill({ contentType: 'text/html', body: `<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1"><style>${style.css}\n${mobileStyles[worksafe]}</style><body class="${worksafe ? 'ws' : 'nws'}"><div id="boardNavDesktop" class="desktop"><span class="boardList">[ ${list} ]</span><span id="navtopright"><a id="settingsWindowLink" href="#">Settings</a></span></div><div id="boardNavMobile" class="mobile"><div class="boardSelect"><strong>Board</strong> <select id="boardSelectMobile"></select></div><div class="pageJump"><a href="#bottom">▼</a><a id="settingsWindowLinkMobile" href="#">Settings</a><a href="/">Home</a></div></div><div class="boardBanner"><div id="bannerCnt" class="title desktop"></div><div class="boardTitle">/demo/ - Owned demo</div><div class="boardSubtitle">Owned description</div></div><main>Owned empty page</main><div id="absbot" class="absBotText"><div id="footer-links"><a href="/about">About</a></div></div><div id="bottom"></div></body>` });
      }
      denied.push(new URL(route.request().url()).origin + new URL(route.request().url()).pathname); return route.abort();
    });
    await page.goto(`${origin}/demo/`);
    await page.evaluate(() => { window.FC = {}; window.style_group = 'ws_style'; localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })); });
    await page.addScriptTag({ content: core });
    await page.evaluate(() => { buildMobileNav(); cloneTopNav(); });
    await page.addScriptTag({ content: extension });
    await page.evaluate(() => { window.$L = { d: () => 'reference.invalid' }; Main.addCSS(); document.body.classList.add('m-dark'); });
    const values = await page.evaluate(({ selectors, properties }) => Object.fromEntries(Object.entries(selectors).map(([name, selector]) => {
      const node = document.querySelector(selector); if (!node) throw new Error(`Missing ${selector}`);
      const computed = getComputedStyle(node); return [name, Object.fromEntries(properties.map(property => [property, computed[property]]))];
    })), { selectors, properties });
    assert.equal(values.page.backgroundImage, 'none'); assert.equal(values.page.color, 'rgb(197, 200, 198)');
    assert.equal(values.mobile.backgroundColor, 'rgb(29, 31, 33)');
    const hover = {};
    for (const [name, selector] of [['mobileLink', selectors.mobileLink], ['footerLinks', '#footer-links a']]) {
      await page.locator(selector).first().hover();
      hover[name] = await page.locator(selector).first().evaluate(node => ({ color: getComputedStyle(node).color,
        borderBottomColor: getComputedStyle(node).borderBottomColor }));
      assert.equal(hover[name].color, name === 'mobileLink' ? 'rgb(95, 137, 172)' : 'rgb(129, 162, 190)');
      await page.mouse.move(0, 850);
    }
    assert.deepEqual(errors, []); assert.ok(denied.length <= 16, 'Unexpected resource volume');
    cases.push({ theme: style.theme, worksafe, width, scale, values, hover }); await page.close();
  }
} finally { await browser.close(); }
const result = { collection_date: '2026-10-01', browser: '151.0.7922.34', sources,
  scope: 'Whole unchanged Core v1128 and extension v1191 loaded after DOMContentLoaded on owned empty mobile board markup. Original buildMobileNav, cloneTopNav and Main.addCSS invoked; the admitted m-dark class supplies the dark setting. Only hostname mapping replaced. Six component color/background/bottom-border properties plus visible mobile/footer-link hover colors. No Main.run, catalog dark mode, public posts/media, banner images, full extension startup or whole-page pixels.', cases };
const target = new URL('docs/public-page-dark-reference.json', root);
if (args[1] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target)));
console.log(`Public mobile dark navigation: ${cases.length} owned component cases match.`);
