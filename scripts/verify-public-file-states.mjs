import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { parseFragment, serializeOuter } from 'parse5';
import { chromium } from '@playwright/test';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <pinned-reference-directory> [--write]');
const json = async path => JSON.parse(await readFile(new URL(path, root)));
const pin = await json('docs/public-watcher-assets.json');
const bytes = await readFile(resolve(args[0], 'extension.1191.js'));
const sha256 = data => createHash('sha256').update(data).digest('hex');
assert.equal(sha256(bytes), pin.source_sha256);
const source = bytes.toString('utf8'), marker = 'Parser.buildHTMLFromJSON=';
assert.equal(source.split(marker).length, 2);
assert.ok(source.includes('del:o+"filedeleted-res"+n'));
assert.ok(source.includes('n=window.devicePixelRatio>=2?"@2x.gif":".gif"'));
const start = source.indexOf(marker) + marker.length;
const formatter = source.slice(start, source.indexOf(',Parser.truncate=function', start));
assert.equal(Buffer.byteLength(formatter), 6986);
const context = vm.createContext({ Main: { board: 'demo', tid: 1, hasMobileLayout: true }, Config: { revealSpoilers: false },
  $L: { d: () => 'reference.invalid' }, Parser: { icons: {}, customSpoiler: {} },
  document: { createElement: () => ({ getElementsByClassName: () => [] }) } });
for (const name of ['decodeSpecialChars', 'encodeSpecialChars']) {
  const marker = `Parser.${name}=function`, start = source.indexOf(marker);
  const next = /,Parser\.[A-Za-z0-9_]+=function/.exec(source.slice(start + marker.length));
  assert.ok(start >= 0 && next);
  new vm.Script(source.slice(start, start + marker.length + next.index)).runInContext(context, { timeout: 1000 });
}
const build = new vm.Script(`(${formatter})`).runInContext(context, { timeout: 1000 });
const attr = (node, key) => node?.attrs?.find(row => row.name === key)?.value ?? null;
const text = node => node?.nodeName === '#text' ? node.value : (node?.childNodes ?? []).map(text).join('');
function find(node, predicate) {
  if (predicate(node)) return node;
  for (const child of node.childNodes ?? []) { const found = find(child, predicate); if (found) return found; }
}
const states = [], dom = new Map();
for (const kind of ['op', 'reply']) for (const state of ['spoiler', 'revealed', 'deleted']) for (const density of [1, 2]) {
  for (const filename of state === 'deleted' ? ['fold'] : ['fold', 'a'.repeat(41)]) {
    context.Config.revealSpoilers = state === 'revealed';
    context.Parser.icons.del = `//s.4cdn.org/image/filedeleted-res${density === 2 ? '@2x' : ''}.gif`;
    const rendered = build({ no: 1001001, resto: kind === 'op' ? 0 : 1001000, name: 'Owned', now: 'fixed', time: 1788868800,
      filename: context.Parser.encodeSpecialChars(filename), ext: '.png', fsize: 2048, w: 600, h: 360,
      tn_w: 250, tn_h: 150, tim: 1001001, md5: 'owned', spoiler: state !== 'deleted' ? 1 : 0,
      filedeleted: state === 'deleted' ? 1 : 0 }, 'demo', false, false);
    const tree = parseFragment(rendered.innerHTML), file = find(tree, node => attr(node, 'class') === 'file');
    const header = find(file, node => attr(node, 'class') === 'fileText');
    const thumb = find(file, node => (attr(node, 'class') || '').split(' ').includes('fileThumb'));
    const image = find(thumb, node => node.tagName === 'img'), link = find(header ?? {}, node => node.tagName === 'a');
    states.push({ kind, state, density, filename, header_id: attr(header, 'id'), header_title: attr(header, 'title'),
      label: text(link), label_title: attr(link, 'title'), thumb_tag: thumb.tagName, thumb_class: attr(thumb, 'class'),
      image_src: attr(image, 'src'), image_class: attr(image, 'class'), image_style: attr(image, 'style'), image_alt: attr(image, 'alt'),
      caption: text(find(thumb, node => attr(node, 'class') === 'mFileInfo mobile')) });
    if (filename === 'fold' && state !== 'revealed') dom.set(`${kind}/${state}/${density}`, serializeOuter(file));
  }
}
const assets = (await json('docs/public-catalog-assets.json')).assets.filter(row => ['spoiler.png', 'filedeleted-res.gif', 'filedeleted-res@2x.gif'].includes(row.name));
const substitutions = [];
for (const asset of assets) {
  const bytes = await readFile(new URL(`apps/public/static/catalog/${asset.name}`, root));
  assert.equal(bytes.length, asset.bytes); assert.equal(sha256(bytes), asset.sha256);
  substitutions.push([`//s.4cdn.org/image/${asset.name}`, `data:${asset.mime};base64,${bytes.toString('base64')}`]);
}
const desktop = await json('docs/public-theme-reference.json'), mobile = await json('docs/public-watcher-navigation-reference.json');
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];
const names = ['yotsubanew', 'yotsubluenew', 'futabanew', 'burichannew', 'photon', 'tomorrow'];
const css = new Map(), styles = [];
for (const name of [...names, 'yotsubamobile', 'yotsubluemobile']) {
  const pin = [...desktop.assets, ...mobile.stylesheets].find(row => row.url.endsWith(`/${name}.716.css`));
  const bytes = await readFile(resolve(args[0], `${name}.716.css`));
  assert.equal(sha256(bytes), pin.sha256); css.set(name, bytes.toString('utf8')); styles.push({ url: pin.url, sha256: pin.sha256 });
}
const browser = await chromium.launch({ headless: true }), cases = [];
try {
  assert.equal(browser.version(), '151.0.7922.34');
  for (const density of [1, 2]) {
    const page = await browser.newPage({ deviceScaleFactor: density });
    await page.route('**/*', route => route.abort());
    for (const family of ['yotsubamobile', 'yotsubluemobile']) for (const viewport of [[1280, 900], [390, 844]]) {
      await page.setViewportSize({ width: viewport[0], height: viewport[1] });
      for (const [index, name] of names.entries()) for (const kind of ['op', 'reply']) for (const state of ['spoiler', 'deleted']) {
        let file = dom.get(`${kind}/${state}/${density}`);
        for (const [remote, local] of substitutions) file = file.replaceAll(remote, local);
        await page.setContent(`<!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1"><style>${css.get(name)}\n${css.get(family)}</style></head><body><div class="board"><div class="thread"><div class="post ${kind}">${file}</div></div></div></body></html>`);
        await page.locator('img').evaluateAll(nodes => Promise.all(nodes.map(node => node.decode())));
        const values = await page.locator('.file').evaluate(file => {
          const values = {};
          for (const [key, selector, properties] of [
            ['thumb', '.fileThumb', ['float', 'margin', 'textDecorationLine']],
            ['image', '.fileThumb img', ['float', 'width', 'height', 'maxWidth', 'maxHeight', 'objectFit']],
          ]) {
            const computed = getComputedStyle(file.querySelector(selector));
            values[key] = Object.fromEntries(properties.map(property => [property, computed[property]]));
          }
          return values;
        });
        cases.push({ density, family, viewport, theme: themes[index], kind, state, values });
      }
    }
    await page.close();
  }
} finally { await browser.close(); }
const result = { collection_date: '2026-09-30', source: pin.source, sha256: pin.source_sha256, browser: '151.0.7922.34',
  scope: 'Isolated public formatter and eight pinned stylesheets with synthetic spoiler/deleted metadata, unchanged fixed assets and denied external requests. No original user media, full-page comparison or board-specific custom spoiler policy.', assets, styles, states, cases };
const target = new URL('docs/public-file-states-reference.json', root);
if (args[1] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target, 'utf8')));
console.log(`Verified ${states.length} released file states and ${cases.length} independent style cases.`);
