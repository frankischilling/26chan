// Whole pinned public core; navigation entry points on owned empty pages only.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

const args = process.argv.slice(2);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <reference-directory> [--write]');
const root = new URL('../', import.meta.url);
const boardManifest = JSON.parse(await readFile(new URL('docs/public-theme-reference.json', root)));
const catalogManifest = JSON.parse(await readFile(new URL('docs/public-catalog-ui-reference.json', root)));
const mobileManifest = JSON.parse(await readFile(new URL('docs/public-watcher-navigation-reference.json', root)));
const observations = JSON.parse(await readFile(new URL('docs/public-page-chrome-dom.json', root)));
const sourcePins = [...boardManifest.assets, ...catalogManifest.styles, ...mobileManifest.stylesheets, boardManifest.client_asset, catalogManifest.client];
const pins = new Map(sourcePins.map(pin => [String(pin.path ?? new URL(pin.url).pathname).split('/').pop(), pin]));
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
async function pinned(name) {
  const pin = pins.get(name);
  assert.ok(pin, `Missing ${name}`);
  const bytes = await readFile(resolve(args[0], name));
  assert.equal(bytes.length, pin.bytes); assert.equal(digest(bytes), pin.sha256);
  return { name, bytes: pin.bytes, sha256: pin.sha256, text: bytes.toString('utf8') };
}
const core = await pinned('core.min.1128.js');
const catalogClient = await pinned('catalog.min.1025.js');
const mobile = await pinned('catalog_mobile.705.css');
const boardMobile = await pinned('yotsubamobile.716.css');
const blueMobile = await pinned('yotsubluemobile.716.css');
const names = {
  yotsuba: ['yotsubanew', 'yotsuba_new'], 'yotsuba-b': ['yotsubluenew', 'yotsuba_b_new'],
  futaba: ['futabanew', 'futaba_new'], burichan: ['burichannew', 'burichan_new'],
  photon: ['photon', 'photon'], tomorrow: ['tomorrow', 'tomorrow'],
};
const styles = [];
for (const [theme, [board, catalog]] of Object.entries(names)) {
  styles.push({ theme, mode: 'index', ...(await pinned(`${board}.716.css`)) });
  styles.push({ theme, mode: 'catalog', ...(await pinned(`catalog_${catalog}.705.css`)), mobile });
}
const ownBoards = [{ slug: 'zed', title: 'Owned zed', nws: true },
  { slug: 'demo', title: 'Paper craft', nws: false }, { slug: 'f', title: 'Owned file board', nws: false }];
function html(style, worksafe) {
  const list = ownBoards.map(row => `<span${row.nws ? ' class="nwsb"' : ''}><a href="/${row.slug}/${style.mode === 'catalog' && row.slug !== 'f' ? 'catalog' : ''}" title="${row.title}">${row.slug}</a></span>`).join(' / ');
  const nav = `<div id="boardNavDesktop" class="desktop"><span class="boardList">[ ${list} ]</span><span id="navtopright">[<a href="#" id="settingsWindowLink">Settings</a>] [<a href="/">Home</a>]</span></div>`;
  const mobileStyle = style.mode === 'catalog' ? mobile : worksafe ? blueMobile : boardMobile;
  // Core changes the desktop switch link; the observed board mobile link stays
  // selected by board safety. Catalog loadCatalog adds its admitted theme name.
  const themeClass = style.mode === 'catalog' ? style.name.slice('catalog_'.length).replace('.705.css', '') : '';
  const current = ownBoards.find(board => board.slug === (worksafe ? 'demo' : 'zed'));
  return `<!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1"><style>${style.text}\n${mobileStyle.text}</style></head><body class="${worksafe ? 'ws' : 'nws'} ${style.mode === 'catalog' ? `is_catalog ${themeClass}` : ''}">
${style.mode === 'catalog' ? `<div id="topnav" class="boardnav">${nav}</div>` : nav}
<div id="boardNavMobile" class="mobile"><div class="boardSelect"><strong>Board</strong> <select id="boardSelectMobile"></select></div><div class="pageJump"><a href="#bottom">▼</a><a href="#" id="settingsWindowLinkMobile">Settings</a><a href="/">Home</a></div></div>
<div class="boardBanner"><div id="bannerCnt" class="title desktop"></div><div class="boardTitle">/${current.slug}/ - ${current.title}</div><div class="boardSubtitle">Owned board description</div></div>
<main id="content"><div style="height:200px">Owned empty page</div></main>
<div id="absbot" class="absBotText"><div id="footer-links"><a href="/about">About</a> • <a href="/feedback">Feedback</a> • <a href="/legal">Legal</a> • <a href="/contact">Contact</a></div></div><div id="bottom"></div></body></html>`;
}
const origin = 'https://boards.reference.invalid';
const browser = await chromium.launch();
const cases = [], behavior = [], styleSelection = [];
async function pageFor(style, width, scale = 1, worksafe = true) {
  const page = await browser.newPage({ viewport: { width, height: 900 }, deviceScaleFactor: scale });
  const errors = [], denied = [], navigation = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => {
    // Core's style initializer is outside this navigation replay. Full pinned
    // theme CSS is inline; this owned placeholder avoids automatic replacement.
    window.FC = {};
  });
  await page.route('**/*', route => {
    const url = new URL(route.request().url());
    if (url.origin === origin && route.request().isNavigationRequest()
      && /^\/(?:demo|zed|f)\/(?:catalog)?$/.test(url.pathname) && !url.search) {
      navigation.push(url.pathname);
      return route.fulfill({ body: html(style, worksafe), contentType: 'text/html' });
    }
    denied.push(url.origin + url.pathname);
    return route.abort();
  });
  const current = worksafe ? 'demo' : 'zed';
  await page.goto(`${origin}/${current}/${style.mode === 'catalog' ? 'catalog' : ''}`);
  // Load after DOMContentLoaded. Do not run advertising, analytics, posting,
  // captcha, banner fetching or page-wide initialization on the fixture.
  await page.addScriptTag({ content: core.text });
  await page.evaluate(current => {
    buildMobileNav(); cloneTopNav();
    // Keep the original selection function and substitute only its hostname
    // mapping, so the owned navigation cannot leave the fixture origin.
    window.$L = { d: () => 'reference.invalid' };
    const select = document.getElementById('boardSelectMobile');
    select.value = current; select.addEventListener('change', onMobileSelectChange);
  }, current);
  return { page, errors, denied, navigation };
}
async function close(context) {
  assert.deepEqual(context.errors, [], 'Public core raised a page error');
  assert.ok(context.denied.length <= 16, 'Unexpected resource volume');
  await context.page.close();
}
async function state(page) {
  return page.evaluate(() => {
    const selectors = { page: 'body', banner: '.boardBanner', desktop: '#boardNavDesktop', desktopLink: '#boardNavDesktop .boardList a',
      mobile: '#boardNavMobile', mobileSelect: '#boardSelectMobile', mobileLink: '#settingsWindowLinkMobile',
      title: '.boardTitle', subtitle: '.boardSubtitle', footer: '#boardNavDesktopFoot', footerLink: '#navbotright a',
      footerLinks: '#footer-links', footerText: '#absbot' };
    const properties = ['display', 'position', 'color', 'backgroundColor', 'fontFamily', 'fontSize', 'fontWeight', 'minHeight',
      'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'marginTop', 'marginBottom', 'marginLeft', 'marginRight',
      'textAlign', 'float', 'letterSpacing', 'lineHeight', 'boxSizing', 'borderBottomWidth', 'borderBottomStyle', 'borderBottomColor'];
    const styles = {};
    for (const [name, selector] of Object.entries(selectors)) {
      const node = document.querySelector(selector); if (!node) throw new Error(`Missing ${selector}`);
      const computed = getComputedStyle(node);
      styles[name] = Object.fromEntries(properties.map(property => [property, computed[property]]));
    }
    return { styles, options: Array.from(document.querySelector('#boardSelectMobile').options)
      .map(option => ({ value: option.value, label: option.textContent, class: option.className })),
    selected: document.querySelector('#boardSelectMobile').value,
    clonedIds: ['boardNavDesktopFoot', 'navbotright', 'settingsWindowLinkBot'].map(id => document.querySelectorAll(`#${id}`).length),
    footerBeforeDisclaimer: document.querySelector('#boardNavDesktopFoot').nextElementSibling.id === 'absbot' };
  });
}
try {
  for (const style of styles) for (const worksafe of [true, false]) for (const width of [390, 480, 481, 1280]) for (const scale of [1, 2]) {
    const context = await pageFor(style, width, scale, worksafe);
    const value = await state(context.page);
    assert.deepEqual(value.options.map(option => option.value), ['demo', 'f', 'zed']);
    assert.deepEqual(value.clonedIds, [1, 1, 1]); assert.equal(value.selected, worksafe ? 'demo' : 'zed');
    assert.equal(value.footerBeforeDisclaimer, true);
    const hover = {};
    for (const [name, selector] of Object.entries({ desktopLink: '#boardNavDesktop .boardList a', mobileLink: '#settingsWindowLinkMobile', footerLink: '#navbotright a' })) {
      let target;
      for (const candidate of await context.page.locator(selector).all()) {
        if (await candidate.isVisible()) { target = candidate; break; }
      }
      if (!target) continue;
      await target.hover();
      hover[name] = await target.evaluate(element => {
        const computed = getComputedStyle(element);
        return Object.fromEntries(['color', 'backgroundColor', 'textDecorationLine', 'borderBottomColor'].map(property => [property, computed[property]]));
      });
      await context.page.mouse.move(0, 850);
    }
    cases.push({ theme: style.theme, mode: style.mode, worksafe, width, scale, ...value, hover });
    await close(context);
  }
  for (const mode of ['index', 'catalog']) for (const board of ['demo', 'zed', 'f']) {
    const style = styles.find(value => value.theme === 'yotsuba' && value.mode === mode);
    const context = await pageFor(style, 390);
    await Promise.all([context.page.waitForEvent('load'), context.page.selectOption('#boardSelectMobile', board)]);
    const path = `/${board}/${mode === 'catalog' && board !== 'f' ? 'catalog' : ''}`;
    await context.page.waitForURL(origin + path);
    assert.equal(context.navigation.at(-1), path);
    assert.equal(context.navigation.length, 2);
    behavior.push({ mode, board, path }); await close(context);
  }
  const labels = { yotsuba: 'Yotsuba New', 'yotsuba-b': 'Yotsuba B New', futaba: 'Futaba New', burichan: 'Burichan New', photon: 'Photon', tomorrow: 'Tomorrow' };
  for (const worksafe of [true, false]) {
    const context = await pageFor(styles.find(style => style.theme === 'yotsuba' && style.mode === 'index'), 390, 1, worksafe);
    const selections = await context.page.evaluate(({ origin, worksafe, labels, paths }) => {
      const create = (title, href) => {
        const link = document.createElement('link'); link.rel = 'alternate stylesheet'; link.title = title; link.href = href; link.disabled = true;
        document.head.append(link); return link;
      };
      for (const [theme, title] of Object.entries(labels)) create(title, `${origin}/css/${paths[theme]}`);
      const selected = create('switch', `${origin}/css/${paths.yotsuba}`);
      const mobile = create('', `${origin}/css/${worksafe ? 'yotsubluemobile.716.css' : 'yotsubamobile.716.css'}`);
      delete window.FC; window.style_group = worksafe ? 'ws_style' : 'nws_style';
      const values = [];
      for (const [theme, title] of Object.entries(labels)) {
        document.cookie = `${style_group}=${encodeURIComponent(title)}; Path=/`;
        initStyleSheet();
        values.push({ theme, worksafe, desktop: selected.href, mobile: mobile.href, mobileConnected: mobile.isConnected });
      }
      localStorage.setItem('4chan_never_show_mobile', 'true'); initStyleSheet();
      return { values, disabledMobileConnected: mobile.isConnected, disabledPreference: localStorage.getItem('4chan_never_show_mobile') };
    }, { origin, worksafe, labels, paths: Object.fromEntries(styles.filter(style => style.mode === 'index').map(style => [style.theme, style.name])) });
    for (const selection of selections.values) {
      const style = styles.find(style => style.theme === selection.theme && style.mode === 'index');
      assert.equal(selection.desktop, `${origin}/css/${style.name}`);
      assert.equal(selection.mobile, `${origin}/css/${worksafe ? blueMobile.name : boardMobile.name}`);
      assert.equal(selection.mobileConnected, true); styleSelection.push(selection);
    }
    assert.equal(selections.disabledMobileConnected, false); assert.equal(selections.disabledPreference, 'true');
    await close(context);
  }
  const componentStyles = {};
  const recordedCases = cases.map(row => ({ ...row, styles: Object.fromEntries(Object.entries(row.styles).map(([name, properties]) => {
    const id = digest(JSON.stringify(properties)).slice(0, 16);
    if (Object.hasOwn(componentStyles, id)) assert.deepEqual(componentStyles[id], properties, 'Recorded style hash collision');
    componentStyles[id] = properties; return [name, id];
  })) }));
  const result = { scope: 'Whole pinned public core loaded; unmodified mobile-list construction, desktop/footer cloning, mobile selection and stylesheet selection entry points invoked. Owned empty pages, titles and board links; no public posts, media or page-wide startup replay. Board mobile CSS follows the observed safety-specific link and Core leaves that link unchanged across desktop themes. Catalog fixtures include the theme class added by the pinned v1025 loadCatalog code. Style records qualify these components and responsive boundaries, not full original-page placement.',
    core: { url: boardManifest.client_asset.url, bytes: core.bytes, sha256: core.sha256 },
    catalog_client: { url: catalogManifest.client.url, bytes: catalogClient.bytes, sha256: catalogClient.sha256,
      scope: 'Pinned input for the catalog theme class supplied by this fixture; full catalog startup is separately qualified by public-catalog-ui-reference.json, not replayed here.' },
    observations: observations.map(({ url, collected_at, bytes, sha256, absent, stylesheets }) => ({ url, collected_at, bytes, sha256, absent, stylesheets })),
    environment: { chromium: browser.version(), viewports: [390, 480, 481, 1280], height: 900, densities: [1, 2], network: 'Every nonfixture request aborted; no original advertising, analytics or banner initialization.' },
    stylesheets: [...new Map([mobile, boardMobile, blueMobile, ...styles].map(({ name, bytes, sha256 }) => [name, { name, bytes, sha256 }])).values()], style_selection: styleSelection, behavior, component_styles: componentStyles, cases: recordedCases };
  const target = new URL('docs/public-page-chrome-reference.json', root);
  if (args[1] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
  else assert.deepEqual(result, JSON.parse(await readFile(target)));
  console.log(`Public page chrome: ${behavior.length} navigation states, ${styleSelection.length} stylesheet selections and ${cases.length} style cases match.`);
} finally { await browser.close(); }
