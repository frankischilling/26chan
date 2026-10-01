import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { chromium } from '@playwright/test';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(args.length === 3 || args.length === 4 && args[3] === '--write',
  'Use <extension.1191.js> <desktop-css-directory> <mobile-css-directory> [--write]');
const json = async path => JSON.parse(await readFile(new URL(path, root), 'utf8'));
const release = await json('docs/public-watcher-assets.json');
const badges = await json('docs/public-capcode-reference.json');
const navigation = await json('docs/public-watcher-navigation-reference.json');
const digest = value => createHash('sha256').update(value).digest('hex');
const source = await readFile(resolve(args[0]));
assert.equal(digest(source), release.source_sha256);
const text = source.toString('utf8'), marker = 'Parser.buildHTMLFromJSON=';
assert.equal(text.split(marker).length, 2);
const start = text.indexOf(marker) + marker.length, end = text.indexOf(',Parser.truncate=function', start);
assert.equal(end - start, 6986);
const formatter = text.slice(start, end);

// Read only the inspected CSS data literal; never run addCSS or initialization.
const cssMarker = 'Main.addCSS=function(){\r\nvar e,t=';
assert.equal(text.split(cssMarker).length, 2);
const cssStart = text.indexOf(cssMarker) + cssMarker.length;
assert.equal(text[cssStart], "'");
let cssEnd = cssStart + 1;
for (; cssEnd < text.length; cssEnd++) {
  if (text[cssEnd] === '\\') { cssEnd++; continue; }
  if (text[cssEnd] === "'") break;
}
assert.ok(cssEnd < text.length);
assert.equal(text.slice(cssEnd + 1, cssEnd + 5), ';(e=');
const extensionCss = new vm.Script(text.slice(cssStart, cssEnd + 1))
  .runInNewContext(Object.create(null), { timeout: 1000 });
assert.equal(typeof extensionCss, 'string');
assert.equal(Buffer.byteLength(extensionCss), 21407);
assert.equal(digest(extensionCss), '4e79f90a330a7b733c35623b227827b92164ee3c44f79b918807c0f3876ca717');
const icons = new Map();
for (const pin of badges.assets) {
  const data = await readFile(new URL(`apps/public/static/identity/${pin.name}`, root));
  assert.equal(digest(data), pin.sha256); assert.equal(data.length, pin.bytes);
  icons.set(`/image/${pin.name}`, data);
}
const styles = ['yotsubanew', 'yotsubluenew', 'futabanew', 'burichannew', 'photon', 'tomorrow'];
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];
const roles = ['mod', 'admin', 'admin_highlight', 'manager', 'developer', 'founder'];
const css = new Map(), pins = [];
for (const [directory, names, references] of [[args[1], styles, badges.styles],
  [args[2], ['yotsubamobile', 'yotsubluemobile'], navigation.stylesheets]]) {
  for (const name of names) {
    const data = await readFile(resolve(directory, `${name}.716.css`));
    const pin = references.find(row => row.url.endsWith(`/${name}.716.css`));
    assert.ok(pin); assert.equal(digest(data), pin.sha256);
    css.set(name, data.toString('utf8')); pins.push({ url: pin.url, sha256: pin.sha256 });
  }
}
const browser = await chromium.launch({ headless: true });
try {
  assert.equal(browser.version(), '151.0.7922.34');
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 1 });
  const unexpected = [];
  await context.route('**/*', async route => {
    const url = new URL(route.request().url());
    if (url.origin === 'https://reference.invalid' && icons.has(url.pathname)) {
      await route.fulfill({ contentType: 'image/gif', body: icons.get(url.pathname) }); return;
    }
    unexpected.push(url.origin + url.pathname); await route.abort();
  });
  const page = await context.newPage(), cases = [];
  for (const dark of [false, true]) for (const mobile of ['yotsubamobile', 'yotsubluemobile']) {
    for (const [index, style] of styles.entries()) for (const capcode of roles) for (const kind of ['op', 'reply']) {
      await page.mouse.move(0, 0);
      await page.setContent(`<!doctype html><html><head><style>${css.get(style)}\n${css.get(mobile)}\n${extensionCss}</style></head><body class="${dark ? 'm-dark' : ''}"><main class="board"><section class="thread"></section></main></body></html>`);
      const values = await page.evaluate(({ formatter, capcode, kind }) => {
        window.Main = { board: 'demo', tid: 0, hasMobileLayout: true };
        window.Config = { revealSpoilers: false };
        window.$L = { d: () => 'reference.invalid' };
        window.Parser = { icons: Object.fromEntries([['mod', 'modicon'], ['admin', 'adminicon'], ['dev', 'developericon'],
          ['manager', 'managericon'], ['founder', 'foundericon']].map(([key, value]) => [key, `https://reference.invalid/image/${value}.gif`])) };
        const build = (0, eval)(`(${formatter})`);
        const post = build({ no: kind === 'op' ? 1001001 : 1001002, resto: kind === 'op' ? 0 : 1001001,
          name: 'Owned staff', sub: 'Owned header subject', capcode, time: 1788868800,
          now: '09/08/26(Tue)08:00:00', com: 'Owned synthetic header text' }, 'demo', kind === 'op', false);
        document.querySelector('.thread').append(post);
        const header = post.querySelector('.postInfoM'), values = {};
        const properties = ['display', 'float', 'clear', 'padding', 'margin', 'borderWidth', 'borderColor',
          'backgroundColor', 'fontSize', 'fontFamily', 'fontWeight', 'lineHeight', 'color', 'textAlign'];
        for (const [key, node] of [['header', header], ['nameBlock', header.querySelector('.nameBlock')],
          ['name', header.querySelector('.name')], ['subject', header.querySelector('.subject')],
          ['badge', header.querySelector('.capcode')], ['icon', header.querySelector('.identityIcon')],
          ['numberDate', header.querySelector('.postNum')], ['post', post.querySelector('.post')]]) {
          if (!node) { values[key] = null; continue; }
          const computed = getComputedStyle(node);
          values[key] = Object.fromEntries(properties.map(property => [property,
            property === 'fontFamily' ? computed[property].toLowerCase() : computed[property]]));
          if (key === 'icon') for (const property of ['width', 'height']) values[key][property] = computed[property];
        }
        values.desktopDisplay = getComputedStyle(post.querySelector('.postInfo.desktop')).display;
        values.links = [...header.querySelectorAll('a')].map((node, index) => ({ text: index ? '$post' : node.textContent,
          title: node.title, color: getComputedStyle(node).color }));
        values.classes = header.className;
        values.order = [...header.children].map(node => ({ tag: node.localName, class: node.className }));
        return values;
      }, { formatter, capcode, kind });
      for (let link = 0; link < 2; link++) {
        const target = page.locator('.postInfoM .postNum > a').nth(link);
        await target.hover(); values.links[link].hover = await target.evaluate(node => getComputedStyle(node).color);
      }
      cases.push({ mobile, dark, theme: themes[index], capcode, kind, values });
    }
  }
  assert.deepEqual(unexpected, []); assert.equal(cases.length, 288);
  const result = {
    scope: 'Portable mobile header properties from the pinned released formatter, six desktop styles, both mobile families and the static extension CSS data. Synthetic short labels, ordinary and dark classes only. No extension initialization, menu, local-time controller, original full-page pixels or original-server name serialization is qualified here.',
    browser: browser.version(), viewport: [390, 844], density: 1,
    client: { url: release.source, sha256: release.source_sha256, formatter_bytes: 6986 },
    extensionCss: { bytes: 21407, sha256: digest(extensionCss) }, styles: pins, cases,
  };
  const target = new URL('docs/public-mobile-header-reference.json', root);
  if (args[3] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
  else assert.deepEqual(result, await json('docs/public-mobile-header-reference.json'));
  await context.close();
  console.log(`Verified ${cases.length} mobile header cases with fixed icons and no external requests.`);
} finally { await browser.close(); }
