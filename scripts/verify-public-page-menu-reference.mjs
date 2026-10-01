// Whole released clients on owned navigation markup; no original board startup.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

const args = process.argv.slice(2), root = new URL('../', import.meta.url);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <reference-directory> [--write]');
const corePin = JSON.parse(await readFile(new URL('docs/public-theme-reference.json', root))).client_asset;
const extensionPin = JSON.parse(await readFile(new URL('docs/public-settings-transfer-reference.json', root)));
const sources = [];
for (const [name, pin] of [['core.min.1128.js', corePin], ['extension.1191.js', extensionPin]]) {
  const bytes = await readFile(resolve(args[0], name));
  assert.equal(bytes.length, pin.bytes); assert.equal(createHash('sha256').update(bytes).digest('hex'), pin.sha256);
  sources.push({ name, bytes: pin.bytes, sha256: pin.sha256, text: bytes.toString('utf8') });
}
const origin = 'https://boards.reference.invalid', cases = [];
const layouts = [
  { name: 'desktop', dropDownNav: false, classicNav: false, mobile: false },
  { name: 'desktop-classic', dropDownNav: true, classicNav: true, mobile: false },
  { name: 'desktop-drop-down', dropDownNav: true, classicNav: false, mobile: false },
  { name: 'mobile', dropDownNav: false, classicNav: false, mobile: true },
  { name: 'mobile-drop-down', dropDownNav: true, classicNav: false, mobile: true },
  { name: 'mobile-classic-preference', dropDownNav: true, classicNav: true, mobile: true },
];
const browser = await chromium.launch();
try {
  assert.equal(browser.version(), '151.0.7922.34');
  for (const mode of ['index', 'catalog']) for (const layout of layouts) {
    const page = await browser.newPage({ viewport: { width: layout.mobile ? 390 : 1280, height: 900 } });
    const errors = [], denied = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/*', route => {
      if (route.request().isNavigationRequest() && route.request().url() === `${origin}/demo/${mode === 'catalog' ? 'catalog' : ''}`) {
        const list = ['zed', 'demo', 'f'].map(board => `<span${board === 'zed' ? ' class="nwsb"' : ''}><a href="/${board}/${mode === 'catalog' && board !== 'f' ? 'catalog' : ''}" title="Owned ${board}">${board}</a></span>`).join(' / ');
        return route.fulfill({ contentType: 'text/html', body: `<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1"><body><div id="boardNavDesktop" class="desktop"><span class="boardList">[ ${list} ]</span><span id="navtopright"><a id="settingsWindowLink" href="#">Settings</a></span></div><div id="boardNavMobile" class="mobile"><div class="boardSelect"><select id="boardSelectMobile"></select></div></div><div id="absbot"></div><div id="bottom"></div></body>` });
      }
      denied.push(new URL(route.request().url()).origin + new URL(route.request().url()).pathname); return route.abort();
    });
    await page.goto(`${origin}/demo/${mode === 'catalog' ? 'catalog' : ''}`);
    await page.evaluate(() => { window.FC = {}; window.style_group = 'ws_style'; localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })); });
    await page.addScriptTag({ content: sources[0].text });
    await page.evaluate(() => { buildMobileNav(); cloneTopNav(); document.getElementById('boardSelectMobile').value = 'demo'; });
    // Main.init executes, while Main.run's DOMContentLoaded listener is too late.
    await page.addScriptTag({ content: sources[1].text });
    const state = await page.evaluate(({ layout, origin }) => {
      window.$L = { d: () => 'reference.invalid' };
      Config.dropDownNav = layout.dropDownNav; Config.classicNav = layout.classicNav; Main.hasMobileLayout = layout.mobile;
      if (layout.dropDownNav && !layout.mobile) Main.initPersistentNav();
      CustomMenu.apply('demo f'); CustomMenu.initCtrl();
      const select = document.getElementById('boardSelectMobile');
      const result = {
        options: [...select.options].map(option => ({ value: option.value, label: option.textContent, class: option.className })), selected: select.value,
        menus: [...document.querySelectorAll('.customBoardList')].map(menu => ({ parent: menu.parentElement.id || menu.parentElement.className,
          links: [...menu.querySelectorAll('a[href]')].map(link => { const url = new URL(link.href); if (url.origin !== origin) throw new Error('Nonfixture menu destination'); return url.pathname; }) })),
        hiddenLists: [...document.querySelectorAll('.boardList')].map(list => list.style.display === 'none'),
        showAll: document.querySelectorAll('.show-all-boards').length,
      };
      CustomMenu.reset(); result.afterReset = { menus: document.querySelectorAll('.customBoardList').length,
        hiddenLists: [...document.querySelectorAll('.boardList')].map(list => list.style.display === 'none') };
      return result;
    }, { layout, origin });
    assert.deepEqual(state.options.map(option => option.value), ['demo', 'f', 'zed']); assert.equal(state.selected, 'demo');
    const separate = layout.dropDownNav && !layout.classicNav && !layout.mobile;
    assert.equal(state.menus.length, separate ? 1 : 2);
    for (const menu of state.menus) assert.deepEqual(menu.links, ['/demo/', '/f/']);
    assert.deepEqual(state.hiddenLists, [!separate, !separate]); assert.equal(state.showAll, separate ? 0 : 2);
    assert.deepEqual(state.afterReset, { menus: 0, hiddenLists: [false, false] });
    assert.deepEqual(errors, []); assert.equal(denied.length, 0);
    cases.push({ mode, ...layout, ...state }); await page.close();
  }
} finally { await browser.close(); }
const result = { collection_date: '2026-10-01', browser: '151.0.7922.34', sources: sources.map(({ name, bytes, sha256 }) => ({ name, bytes, sha256 })),
  scope: 'Whole unchanged Core v1128 and extension v1191 loaded after DOMContentLoaded on owned navigation markup. Original buildMobileNav, cloneTopNav, Main.initPersistentNav, CustomMenu.apply/initCtrl/reset entry points invoked. Only the hostname mapping is replaced. No Main.run, public posts/media, complete menu styling, editor placement or original-page startup qualification.', cases };
const target = new URL('docs/public-page-menu-reference.json', root);
if (args[1] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target)));
console.log(`Public menu reference: ${cases.length} owned navigation cases match.`);
