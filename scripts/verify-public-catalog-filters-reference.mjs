// Whole pinned catalog client on owned data and static selector shape only.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { chromium } from '@playwright/test';

const args = process.argv.slice(2);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <pinned-reference-directory> [--write]');
const root = new URL('../', import.meta.url);
const manifest = JSON.parse(await readFile(new URL('docs/public-catalog-reference.json', root)));
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
async function pinned(name) {
  const pin = manifest.assets.find(asset => asset.path.endsWith('/' + name));
  assert.ok(pin, name);
  const bytes = await readFile(resolve(args[0], name));
  assert.equal(bytes.length, pin.bytes); assert.equal(digest(bytes), pin.sha256);
  return { pin, text: bytes.toString('utf8') };
}
const client = await pinned('catalog.min.1025.js');
const mobile = await pinned('catalog_mobile.705.css');
const themes = Object.entries(manifest.themes);
const styles = await Promise.all(themes.map(async ([theme, name]) => ({ theme, name, ...(await pinned(`catalog_${name}.705.css`)) })));
const controlsAssets = JSON.parse(await readFile(new URL('docs/public-catalog-control-assets.json', root)));
for (const asset of controlsAssets) {
  const bytes = await readFile(resolve(args[0], new URL(asset.url).pathname.split('/').pop()));
  assert.equal(bytes.length, asset.bytes); assert.equal(digest(bytes), asset.sha256);
  assert.ok(bytes.equals(await readFile(new URL(asset.path, root))));
}
const icons = JSON.parse(await readFile(new URL('docs/public-catalog-filter-assets.json', root)));
for (const asset of icons.assets) {
  const bytes = await readFile(resolve(args[0], 'catalog-filter-' + asset.name.replace('/', '-')));
  assert.equal(bytes.length, asset.bytes); assert.equal(digest(bytes), asset.sha256);
  assert.ok(bytes.equals(await readFile(new URL('apps/public' + icons.local_base + asset.name, root))));
}
const catalog = { slug: 'demo', anon: 'Anonymous', count: 3, flags: false, threads: {
  1000001: { author: "Avery", trip: "!Origami", b: 2, sub: 'Owned crane', teaser: 'One sheet', file: 'crane.png', s: 1, r: 2, i: 0, date: 1788868800, w: 1, h: 1, tn_w: 1, tn_h: 1, lr: {} },
  1000002: { author: "Riley", trip: "!Paper", capcode: "mod", b: 1, sub: 'Owned boat', teaser: 'Two folds', file: 'boat.png', s: 2, r: 1, i: 0, date: 1788868800, w: 1, h: 1, tn_w: 1, tn_h: 1, lr: {} },
  1000003: { author: "Anonymous", sub: "Feeling fold", teaser: "paper feeling", b: 0, s: 3, r: 0, i: 0, date: 1788868800, w: 1, h: 1, tn_w: 1, tn_h: 1, lr: {} },
}, order: { alt: [1000001, 1000002], absdate: [1000001, 1000002], date: [1000002, 1000001], r: [1000001, 1000002] } };
const auxiliary = ['settingsWindowLink', 'settingsWindowLinkBot', 'settingsWindowLinkMobile', 'togglePostFormLinkMobile',
  'filtered-label', 'hidden-label', 'filtered-label-bottom', 'hidden-label-bottom', 'filtered-count', 'filtered-count-bottom',
  'hidden-count', 'hidden-count-bottom', 'ordered-by', 'last-updated', 'last-updated-bottom', 'filters-clear-hidden', 'filters-clear-hidden-bottom'];
function html(css) {
  return `<!doctype html><meta name="viewport" content="width=device-width, initial-scale=1"><style>${css}\n${mobile.text}</style><link id="mobile-css">
<div id="backdrop" class="hidden"></div><div id="boardNavDesktop"></div><div id="boardNavDesktopFoot"></div><div id="boardNavMobile"></div>
<form name="post"><table id="postForm"></table></form><div id="ctrl"><div id="info">
<span id="search-label">Search: <span id="search-term"></span></span></div><hr class="mobile">
<div id="settings" class="mobilebtn"><span class="ctrl-wrap">Sort by: <select id="order-ctrl"><option value="alt">Bump order</option><option value="absdate">Last reply</option><option value="date">Creation date</option><option value="r">Reply count</option></select></span>
<span class="ctrl-wrap">Image size: <select id="size-ctrl"><option value="small">Small</option><option value="large">Large</option></select></span>
<span class="ctrl-wrap">Teasers: <select id="teaser-ctrl"><option value="off">Off</option><option value="on">On</option></select></span>
<span class="btn-wrap"><span id="filters-ctrl" class="button">Filters</span></span>
<span class="btn-wrap"><span id="qf-ctrl" class="button">Search</span></span><span id="qf-cnt"><input id="qf-box" name="qf-box" type="text"><span id="qf-clear" class="button">×</span></span>
</div><div class="clear"></div></div><hr><div id="threads"></div><span id="search-label-bottom">Search: <span id="search-term-bottom"></span></span>
${auxiliary.map(id => `<span id="${id}"></span>`).join('')}
<select id="styleSelector"><option value="Yotsuba B New">Yotsuba B</option></select><div id="bottom"></div>`;
}
const browser = await chromium.launch();
const pageErrors = new WeakMap();
async function closePage(page) {
  assert.deepEqual(pageErrors.get(page), [], 'Pinned catalog client raised a page error');
  await page.close();
}
async function pageFor(style, width, hash = '', stored = {}, density = 1) {
  const page = await browser.newPage({ viewport: { width, height: 900 }, deviceScaleFactor: density });
  const errors = [];
  pageErrors.set(page, errors);
  page.on('pageerror', error => errors.push(error.stack));
  await page.route('**/*', route => {
    const url = new URL(route.request().url());
    return url.origin === 'https://reference.invalid' && url.pathname === '/' && !url.search
      && route.request().isNavigationRequest()
      ? route.fulfill({ body: html(style.text), contentType: 'text/html' }) : route.abort();
  });
  await page.goto('https://reference.invalid/' + hash);
  await page.clock.install({ time: new Date('2026-09-08T12:00:00Z') });
  await page.evaluate(({ name, stored }) => {
    localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
    document.cookie = `ws_style=${encodeURIComponent(name.replaceAll('_', ' ').replace(/\b[a-z]/g, value => value.toUpperCase()))}; Path=/`;
    for (const [key, value] of Object.entries(stored)) { if (key.startsWith('4chan-catalog-search')) sessionStorage.setItem(key, value); else localStorage.setItem(key, typeof value === 'string' ? value : JSON.stringify(value)); }
  }, { name: style.name, stored });
  await page.addScriptTag({ content: client.text });
  await page.evaluate(catalog => {
    window.$L = { d: () => 'reference.invalid' };
    window.fourcat = new FC();
    fourcat.applyCSS(null, 'ws_style', 705);
    fourcat.init(); fourcat.loadCatalog(catalog);
  }, catalog);
  await page.clock.pauseAt(new Date('2026-09-08T13:00:00Z'));
  assert.deepEqual(errors, []);
  return page;
}

const rule = (pattern, extra = {}) => ({ active: 1, pattern, color: "#E0B0FF", boards: '', hidden: 0, top: 0, ...extra });
const filterObject = rules => Object.fromEntries(rules.map((value, index) => [index, value]));
const vectors = [
  ['whole-word', [rule('sheet')]], ['case-insensitive', [rule('SHEET')]],
  ['whole-word-not-prefix', [rule('feel')]], ['and', [rule('owned sheet')]],
  ['or', [rule('sheet|folds')]], ['or-spacing', [rule('sheet | folds')]],
  ['mixed', [rule('owned sheet|folds')]], ['wildcard-suffix', [rule('feel*')]],
  ['wildcard-middle', [rule('f*lds')]], ['phrase-case-sensitive', [rule('"One sheet"')]],
  ['phrase-case-mismatch', [rule('"one sheet"')]], ['regex', [rule('/one sheet/i')]],
  ['regex-case-sensitive', [rule('/one sheet/')]], ['filename', [rule('boat.png')]],
  ['absent-filename-coercion', [rule('/^undefined$/')]], ['tripcode', [rule('#!Origami')]],
  ['tripcode-substring', [rule('#Origami')]], ['capcode', [rule('#!#mod')]],
  ['name', [rule('##Riley')]], ['name-case-sensitive', [rule('##riley')]],
  ['boards-match', [rule('sheet', { boards: 'demo' })]], ['boards-other', [rule('sheet', { boards: 'other' })]],
  ['boards-spaces', [rule('sheet', { boards: 'other demo' })]], ['boards-comma-literal', [rule('sheet', { boards: 'other,demo' })]],
  ['inactive', [rule('sheet', { active: 0 })]], ['empty', [rule('')]],
  ['hide', [rule('sheet', { hidden: 1 })]], ['top', [rule('folds', { top: 1 })]],
  ['highlight', [rule('sheet', { color: '#E0B0FF' })]],
  ['first-match', [rule('sheet', { color: '#FFFF00' }), rule('sheet', { hidden: 1 })]],
  ['first-hide', [rule('sheet', { hidden: 1 }), rule('sheet', { color: '#FFFF00' })]],
  ['pinned-bypass', [rule('sheet', { hidden: 1 })], { '4chan-pin-demo': { 1000001: 2 } }],
  ['hidden-bypass', [rule('sheet', { color: '#FFFF00' })], { '4chan-hide-t-demo': { 1000001: 1 } }],
  ['search-bypass', [rule('sheet', { hidden: 1 })], { '4chan-catalog-search': 'sheet', '4chan-catalog-search-board': 'demo' }],
];
async function catalogState(page) {
  assert.deepEqual(pageErrors.get(page), []);
  return page.evaluate(() => ({
    cards: [...document.querySelectorAll('#threads > .thread')].map(node => ({ id: node.id,
      highlighted: node.querySelector('.thumb').classList.contains('hl'),
      border: node.querySelector('.thumb').style.borderColor,
      color: node.querySelector('.teaser')?.style.color ?? '',
    })),
    filtered: ['filtered-label', 'filtered-label-bottom'].map(id => getComputedStyle(document.getElementById(id)).display),
    counts: ['filtered-count', 'filtered-count-bottom'].map(id => document.getElementById(id).textContent),
  }));
}
async function editorState(page, label, states) {
  assert.deepEqual(pageErrors.get(page), [], label);
  const value = await page.evaluate(() => ({
    open: !document.getElementById('filters').classList.contains('hidden'),
    backdrop: !document.getElementById('backdrop').classList.contains('hidden'),
    search: document.getElementById('filters-search').value,
    rows: [...document.getElementById('filter-list').children].map(row => ({
      pattern: row.querySelector('.filter-pattern').value, boards: row.querySelector('.filter-boards').value,
      active: row.querySelector('.filter-active').checked, hidden: row.querySelector('.filter-hide').checked,
      top: row.querySelector('.filter-top').checked, display: row.style.display,
      color: row.querySelector('.filter-color').style.backgroundColor,
      noColor: row.querySelector('.filter-color').hasAttribute('data-nocolor'),
      hits: row.querySelector('.filter-hits').textContent,
    })),
    palette: !document.getElementById('filter-palette').classList.contains('hidden'),
    help: !document.getElementById('filters-protip').classList.contains('hidden'),
    stored: localStorage.getItem('catalog-filters'),
  }));
  states.push({ label, width: page.viewportSize().width, ...value });
}
async function editorStyle(page) {
  return page.evaluate(() => Object.fromEntries([
    ['panel','#filters',['width','padding','fontSize','backgroundColor','boxShadow','marginLeft']],
    ['header','#filters > .panelHeader',['borderBottomColor','fontSize','fontWeight','marginBottom','marginTop','paddingBottom','textAlign','lineHeight']],
    ['search','#filters-search',['display','width','fontSize','padding']],
    ['table','#filter-table',['width']], ['heading','#filter-table th',['fontSize','minWidth','textAlign']],
    ['cell','#filter-list td',['padding','textAlign']], ['pattern','.filter-pattern',['width','fontSize','padding']],
    ['boards','.filter-boards',['width','fontSize','padding']], ['hits','.filter-hits',['fontSize']], ['help','#filters-help-open',['width','height','backgroundImage','backgroundSize','right','top','marginTop','marginBottom']],
  ].map(([name,selector,properties])=> { const style=getComputedStyle(document.querySelector(selector));
    return [name,Object.fromEntries(properties.map(key=>[key,style[key]]))]; })));
}
const matches=[], states=[], cases=[];
try {
  const style=styles.find(row=>row.theme==='yotsuba-b');
  for(const [label,rules,stored={}] of vectors) {
    const filters=filterObject(rules);
    const page=await pageFor(style,1280,'',{ 'catalog-filters': filters, ...stored });
    matches.push({ label, filters, stored, value: await catalogState(page) });
    await closePage(page);
  }
  for(const width of [1280,390]) {
    const page=await pageFor(style,width);
    await page.locator('#filters-ctrl').click(); await editorState(page,'empty-open',states);
    await page.locator('#filters-add').click(); await editorState(page,'added-empty',states);
    await page.locator('.filter-pattern').fill('sheet'); await page.locator('.filter-boards').fill('demo');
    await page.locator('.filter-hide').check(); await page.locator('.filter-top').check();
    await editorState(page,'both-hide-top',states);
    await page.locator('.filter-color').click(); await editorState(page,'palette-open',states);
    await page.locator('#filter-color-table tbody .clickbox').first().click(); await editorState(page,'palette-selected',states);
    await page.locator('#filters-save').click(); await editorState(page,'saved-closed',states);
    await page.locator('#filters-ctrl').click(); await editorState(page,'reopened-saved',states);
    await page.locator('#filters-add').click(); await page.locator('.filter-pattern').last().fill('folds');
    await editorState(page,'second-added',states);
    await page.locator('[data-up]').last().click(); await editorState(page,'moved-up',states);
    if(width===1280) {
      await page.locator('#filters-search').fill('sheet'); await page.locator('#filters-search').press('ArrowRight');
      await editorState(page,'row-search',states); await page.locator('#filters-search').press('Escape');
      await editorState(page,'row-search-escape',states);
    }
    await page.locator('[data-target]').first().click(); await editorState(page,'deleted-first',states);
    await page.locator('#filters-help-open').click(); await editorState(page,'help-open',states);
    await page.locator('#filters-help-close').click(); await editorState(page,'help-closed',states);
    await page.locator('#filters-close').click(); await editorState(page,'dismissed',states);
    await closePage(page);
  }
  for(const style of styles) for(const width of [1280,390]) for(const density of [1,2]) {
    const page=await pageFor(style,width,'',{ 'catalog-filters': filterObject([rule('sheet',{color:'#E0B0FF'})]) },density);
    await page.locator('#filters-ctrl').click(); await page.mouse.move(0,0);
    cases.push({ theme:style.theme,width,density,values:await editorStyle(page) });
    await closePage(page);
  }
} finally { await browser.close(); }
const observed={ scope:'Unchanged pinned public catalog client on three owned cards. No external browser requests. Catalog filters are separate from native extension filters.',
  client:{url:'https://s.4cdn.org/'+client.pin.path,bytes:client.pin.bytes,sha256:client.pin.sha256},
  environment:{chromium:browser.version(),densities:[1,2],viewports:[[1280,900],[390,900]],clock:'2026-09-08T12:00:00Z'},
  styles:[...styles.map(row=>row.pin),mobile.pin],icons,catalog,matches,states,cases };
const destination=new URL('docs/public-catalog-filters-reference.json',root);
if(args[1]==='--write')await writeFile(destination,JSON.stringify(observed,null,2)+'\n');
else assert.deepEqual(observed,JSON.parse(await readFile(destination)));
console.log('PASS '+matches.length+' catalog filter vectors, '+states.length+' editor states and '+cases.length+' style cases');
