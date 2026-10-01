import test from 'node:test';
import assert from 'node:assert/strict';
import { readCatalogTheme, writeCatalogTheme, catalogDropDownEnabled, CATALOG_THEME_LIMITS } from '../../apps/public/static/native-settings.v1.js';
import { parseCatalogCSS, parseCustomCSS } from '../../apps/public/static/native-custom-css.v1.js';

test('catalog flags retain the public sparse object and clearing removes its storage value', () => {
  assert.deepEqual(writeCatalogTheme({ nobinds: true, nospoiler: true, newtab: true }).raw,
    '{"nobinds":true,"nospoiler":true,"newtab":true}');
  assert.equal(writeCatalogTheme({ nobinds: false, nospoiler: false, newtab: false, css: '' }).raw, null);
  assert.deepEqual(readCatalogTheme(null), { status: 'ok', theme: {} });
});

test('catalog navigation defaults distinguish missing storage from an empty admitted object', () => {
  assert.equal(catalogDropDownEnabled(null, false), false);
  assert.equal(catalogDropDownEnabled('{}', false), true);
  assert.equal(catalogDropDownEnabled('{}', true), false);
  assert.equal(catalogDropDownEnabled('{"dropDownNav":false}', false), false);
  assert.equal(catalogDropDownEnabled('{"dropDownNav":true}', true), false);
  assert.equal(catalogDropDownEnabled('{"disableAll":true}', false), false);
  for (const raw of ['null', '[]', '{', '{"__proto__":{}}', ' '.repeat(4097)]) assert.equal(catalogDropDownEnabled(raw, false), false);
});

test('catalog preference admission rejects malformed, reserved and unknown fields within fixed bounds', () => {
  for (const raw of ['null', '[]', '{', 'true', '{"__proto__":{}}', '{"unknown":true}',
    '{"nobinds":1}', '{"nospoiler":"true"}', '{"newtab":null}', '{"css":{}}',
    'x'.repeat(CATALOG_THEME_LIMITS.storage + 1), JSON.stringify({ css: ' '.repeat(CATALOG_THEME_LIMITS.css + 1) })]) {
    assert.equal(readCatalogTheme(raw).status, 'invalid', raw.slice(0, 80));
  }
  const cyclic = {}; cyclic.css = cyclic;
  assert.equal(writeCatalogTheme(cyclic).status, 'invalid');
});

test('the recorded catalog CSS remains text while the compiled stylesheet is confined to catalog cards', () => {
  const css = '.teaser { color: #008000; }';
  const checked = writeCatalogTheme({ css });
  assert.equal(checked.theme.css, css);
  assert.equal(checked.styles.css, '#threads > .thread .teaser { color: #008000; }');
  assert.equal(parseCustomCSS(css).status, 'invalid');
  assert.equal(parseCatalogCSS('.post { color: #008000; }').status, 'invalid');
});

test('invalid stored CSS leaves unrelated catalog flags admitted but cannot be saved or compiled', () => {
  const checked = readCatalogTheme('{"nospoiler":true,"css":"body{display:none}"}');
  assert.equal(checked.status, 'ok'); assert.equal(checked.theme.nospoiler, true);
  assert.equal(typeof checked.cssError, 'string'); assert.equal(checked.styles, undefined);
  assert.equal(writeCatalogTheme(checked.theme).status, 'invalid');
});

test('catalog styles cannot load resources, hide controls, change positioning or escape their selector set', () => {
  for (const css of ['body { color: #fff; }', '#theme { color: #fff; }', '.thread button { color: #fff; }',
    '.teaser { display: none; }', '.thread { position: fixed; }', '.thread { margin: -20px; }',
    '.thumb { background-image: url(https://example.invalid/private); }', '@import "https://example.invalid/private";',
    '.teaser { color: var(--private); }', '.teaser { color: #fff !important; }', '.teaser\\2c body { color: #fff; }',
    '.teaser { color: #fff; } body { display: none; }']) assert.equal(parseCatalogCSS(css).status, 'invalid', css);
});

test('catalog styles enforce actual UTF-8 bytes, rule count and declaration count', () => {
  assert.equal(parseCatalogCSS('\u2000'.repeat(5500)).status, 'invalid');
  assert.equal(parseCatalogCSS('.teaser { color: #fff; }'.repeat(65)).status, 'invalid');
  assert.equal(parseCatalogCSS('.teaser { color: #fff; color: #000; }').status, 'invalid');
  assert.equal(parseCatalogCSS('.txt-sub, .txt-rep { font-size: 14px; padding: 2px; }').status, 'ok');
});
