import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
import { sourceSpoilerPath, configureSpoilerPreference } from '../../apps/public/client/native-spoilers.js';
import { parsePostRecipe, UPDATER_LIMITS } from '../../apps/public/client/native-updater-snapshot.js';
import { isSpoilerAssetPath } from '../../apps/public/client/native-spoiler-assets.js';

const reference = JSON.parse(await readFile(new URL('../../apps/public/tests/fixtures/custom-spoilers.json', import.meta.url)));
const manifest = JSON.parse(await readFile(new URL('../../docs/custom-spoiler-assets.json', import.meta.url)));
const client = JSON.parse(await readFile(new URL('../../apps/public/tests/fixtures/custom-spoiler-clients.json', import.meta.url)));
const pin = value => createHash('sha256').update(value).digest('hex');
const selection = client.native_selection, catalogSelection = client.catalog_selection;
function original(board, primary, random) {
  const context = vm.createContext({ Parser: { customSpoiler: {} }, Main: { board },
    $: { cls: () => primary ? [{ firstChild: { src: 'https://s.4cdn.org/image/' + primary } }] : [] },
    Math: Object.assign(Object.create(Math), { random: () => random }) });
  vm.runInContext(selection, context, { timeout: 1000 });
  return (target, count) => {
    context.Parser.setCustomSpoiler(target, count);
    return `/static/catalog/spoiler${context.Parser.customSpoiler[target] ?? ''}.png`;
  };
}
function document(board, primary) {
  return { getElementById: () => ({ getAttribute: () => board }),
    querySelector: () => primary ? { getAttribute: () => '/static/catalog/' + primary } : null };
}
test('native per-board caching and HTML aliases match the actual pinned source function', () => {
  const saved = Math.random;
  try {
    for (const row of reference.boards.filter(row => row.count)) {
      for (const random of [0, 0.5, 0.999999]) {
        Math.random = () => random;
        const expected = original('other', null, random), doc = document('other', null);
        for (const count of [row.count, 0, row.count === 1 ? 6 : 1, row.count]) {
          assert.equal(sourceSpoilerPath(doc, row.board, count), expected(row.board, count), `${row.board}/${count}/${random}`);
        }
        for (const url of row.source_html_urls) {
          const name = url.split('/').at(-1), doc = document(row.board, name), expected = original(row.board, name, random);
          assert.equal(sourceSpoilerPath(doc, row.board, row.count), expected(row.board, row.count));
          assert.equal(sourceSpoilerPath(doc, row.board, 0), expected(row.board, 0));
        }
      }
    }
  } finally { Math.random = saved; }
});
test('catalog chooses the source count suffix, independently of server HTML aliases', () => {
  for (const row of reference.boards) {
    const context = vm.createContext({ catalog: { slug: row.board, custom_spoiler: row.enabled ? row.count : 0 },
      options: { imgspoiler: '/static/catalog/spoiler' } });
    vm.runInContext(catalogSelection, context, { timeout: 1000 });
    assert.equal(context.spoiler, row.enabled && row.count ? `/static/catalog/spoiler-${row.board}${row.count}.png` : '/static/catalog/spoiler.png');
  }
});
test('reveal preference suppresses first choice, document caches are shared across bundles and reset on navigation', async () => {
  const otherBundle = await import('../../apps/public/client/native-spoilers.js?independent-bundle');
  const doc = document('m', 'spoiler-m3.png');
  configureSpoilerPreference(doc, () => true);
  assert.equal(sourceSpoilerPath(doc, 'm', 4), '/static/catalog/spoiler.png');
  configureSpoilerPreference(doc, () => false);
  assert.equal(otherBundle.sourceSpoilerPath(doc, 'm', 4), '/static/catalog/spoiler-m3.png');
  assert.equal(sourceSpoilerPath(doc, 'm', 1), '/static/catalog/spoiler-m3.png');
  assert.equal(sourceSpoilerPath(document('m', 'spoiler-m1.png'), 'm', 4), '/static/catalog/spoiler-m1.png');
});
test('fixed UI assets match collected bytes and dimensions, including the two explicitly unavailable names', async () => {
  assert.deepEqual(manifest.assets.filter(row => row.status !== 200).map(row => row.name), ['spoiler-news1.png', 'spoiler-vm1.png']);
  for (const row of manifest.assets) {
    assert.ok(isSpoilerAssetPath('/static/catalog/' + row.name));
    if (row.status !== 200) continue;
    const bytes = await readFile(new URL('../../apps/public/static/catalog/' + row.name, import.meta.url));
    assert.equal(bytes.length, row.bytes); assert.equal(pin(bytes), row.sha256);
    assert.equal(bytes.readUInt32BE(16), 100); assert.equal(bytes.readUInt32BE(20), 100);
  }
  for (const raw of ['/static/catalog/spoiler-x1.png', '/static/catalog/../spoiler-m1.png', 'https://s.4cdn.org/image/spoiler-m1.png', '/static/catalog/spoiler-m1.png?x']) assert.equal(isSpoilerAssetPath(raw), false);
});
test('custom metadata is canonical and root-only, and variants retain the complete file grammar', () => {
  const context = { board: 'm', thread: '1', origin: 'https://board.test', mediaOrigin: 'https://media.test' };
  const html = `<article class="postContainer opContainer" id="pc1" data-custom-spoiler="4"><div class="post op" id="p1"><div class="postInfo" id="pi1"><span class="name">Owned</span></div><div class="file" id="f1"><div class="fileText" id="fT1" title="Owned.png">File: <a href="https://media.test/m/1.png" rel="noopener noreferrer">Spoiler Image</a> (2 KB, 600x360)</div><a class="fileThumb imgspoiler" href="https://media.test/m/1.png" rel="noopener noreferrer"><img src="/static/catalog/spoiler-m4.png" alt="2 KB" width="100" height="100" loading="lazy"><div class="mFileInfo mobile">2 KB PNG</div></a></div><blockquote id="m1" class="postMessage">Owned</blockquote></div></article>`;
  for (const count of ['1', '4', '64']) assert.doesNotThrow(() => parsePostRecipe(html.replace('data-custom-spoiler="4"', `data-custom-spoiler="${count}"`), context, '1', { nodes: 0 }, UPDATER_LIMITS));
  for (const value of ['0', '01', '65', '-1', '1.0', '1e0', ' 1', '']) assert.throws(() => parsePostRecipe(html.replace('data-custom-spoiler="4"', `data-custom-spoiler="${value}"`), context, '1', { nodes: 0 }, UPDATER_LIMITS));
  for (const changed of [html.replace('id="pi1"', 'id="pi1" data-custom-spoiler="4"'), html.replace('width="100"', 'width="99"'), html.replace('spoiler-m4.png', 'spoiler-unknown1.png')]) assert.throws(() => parsePostRecipe(changed, context, '1', { nodes: 0 }, UPDATER_LIMITS));
});
