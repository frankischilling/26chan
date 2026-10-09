import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { subtitleSource as source, expectedSubtitle, assertSubtitleDocument } from './source-board-subtitles-contract.mjs';
import { subtitleDatabaseEnvironment } from './source-board-subtitles-fixture.mjs';

const hash = value => createHash('sha256').update(value).digest('hex');
const pins = [...source.boards.map(row => row.source), ...Object.values(source.excerpts), ...source.styles.map(row => row.source)];
test('source subtitle excerpts retain their byte offsets and SHA-256 pins', async () => {
  assert.equal(source.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
  for (const pin of pins) {
    assert.equal(Buffer.byteLength(pin.text), pin.byte_end - pin.byte_start, pin.path);
    assert.equal(hash(pin.text), pin.sha256, pin.path);
    assert.match(pin.source_sha256, /^[a-f0-9]{64}$/);
    if (process.env.SUBTITLE_REFERENCE_DIR) {
      const bytes = await readFile(path.join(process.env.SUBTITLE_REFERENCE_DIR, pin.path));
      assert.equal(hash(bytes), pin.source_sha256, pin.path);
      assert.equal(bytes.subarray(pin.byte_start, pin.byte_end).toString(), pin.text, pin.path);
    }
  }
});
test('only b/trash and gif declare fixed source subtitle content', () => {
  assert.deepEqual(source.boards.map(row => row.board), ['b', 'gif', 'trash']);
  const fiction = 'The stories and information posted here are artistic works of fiction and falsehood.<br>Only a fool would take anything posted here as fact.';
  assert.equal(source.boards.find(row => row.board === 'b').html, fiction);
  assert.equal(source.boards.find(row => row.board === 'trash').html, fiction);
  assert.equal(source.boards.find(row => row.board === 'gif').html,
    'Worksafe Board: /<a href="//boards.4chan.org/wsg/" title="Worksafe GIF">wsg</a>/');
  assert.equal(expectedSubtitle('worksafe_gif'), 'Worksafe Board: /<a href="/wsg/" title="Worksafe GIF">wsg</a>/');
  assert.equal(expectedSubtitle('none'), '');
});
test('common header includes archive index and text layout, alongside the separate catalog header', () => {
  for (const name of ['board_builder', 'catalog_builder']) {
    assert.match(source.excerpts[name].text, /if\( defined\( 'SUBTITLE' \) \)/);
    assert.match(source.excerpts[name].text, /'<div class="boardSubtitle">' \. SUBTITLE \. '<\/div>'/);
  }
  for (const name of ['board_banner', 'catalog_banner']) {
    assert.match(source.excerpts[name].text, /<div class="boardTitle">\$title<\/div>\s+\$subtitle\s+<\/div>/);
  }
  assert.match(source.excerpts.archive_header.text, /head\(\$html, 0, 0, 0, 0, true\)/);
  assert.match(source.excerpts.board_modes.text, /is_arclist/);
  assert.match(source.excerpts.board_modes.text, /if \(TEXT_ONLY\)/);
  assert.doesNotMatch(source.excerpts.board_modes.text, /\$subtitle/);
  assert.match(source.excerpts.catalog_text_mode.text, /\$body_text_css = ' text_only'/);
  assert.doesNotMatch(source.excerpts.catalog_text_mode.text, /\$subtitle/);
});
test('desktop and catalog subtitle type size follows each source theme', async () => {
  for (const row of source.styles) {
    const expected = row.mode === 'text' || ['futaba', 'burichan'].includes(row.theme) ? '10pt' : 'x-small';
    assert.match(row.source.text, new RegExp(`font-size: ${expected};`));
  }
  const chrome = JSON.parse(await readFile(new URL('../../docs/public-page-chrome-reference.json', import.meta.url)));
  for (const [key, styles] of Object.entries(chrome.component_styles)) {
    assert.equal(hash(JSON.stringify(styles)).slice(0, 16), key);
  }
  for (const row of chrome.cases) {
    assert.ok(chrome.component_styles[row.styles.subtitle]);
    assert.equal(chrome.component_styles[row.styles.subtitle].textAlign, 'center');
  }
});
function documentFor(kind, html = expectedSubtitle(kind)) {
  return `<!doctype html><body><div class="boardBanner"><div class="boardTitle">/owned/ - Owned &lt;title&gt;</div>${html ? `<div class="boardSubtitle">${html}</div>` : ''}</div></body>`;
}
test('semantic oracle rejects arbitrary descriptions, escaped source markup and active attributes', () => {
  const board = { kind: 'fiction', slug: 'owned', title: 'Owned <title>', description: 'Unrelated description', textOnly: false };
  for (const kind of ['none', 'fiction', 'worksafe_gif']) {
    assert.doesNotThrow(() => assertSubtitleDocument(documentFor(kind), { ...board, kind }, 'index'));
  }
  for (const html of ['Unrelated description', expectedSubtitle('fiction').replace('<br>', '&lt;br&gt;'),
    `${expectedSubtitle('fiction')}<img src=x onerror=bad()>`]) {
    assert.throws(() => assertSubtitleDocument(documentFor('fiction', html), board, 'index'));
  }
  assert.throws(() => assertSubtitleDocument(documentFor('none', 'Unrelated description'), { ...board, kind: 'none' }, 'index'));
  assert.throws(() => assertSubtitleDocument(documentFor('worksafe_gif', expectedSubtitle('worksafe_gif').replace('title="Worksafe GIF"', 'title="Worksafe GIF" onclick="bad()"')), { ...board, kind: 'worksafe_gif' }, 'index'));
});
test('persisted fixture rejects remote, production, visual and wrong-role databases before mutation', () => {
  const valid = { APP_ENV: 'development', MIGRATION_DATABASE_URL: 'postgres://board_migrator:owned@127.0.0.1/subtitle_test' };
  assert.equal(subtitleDatabaseEnvironment(valid).PGDATABASE, 'subtitle_test');
  for (const change of [{ APP_ENV: 'production' }, { VISUAL_FIXTURE_SERVER: '1' },
    { MIGRATION_DATABASE_URL: 'postgres://board_migrator:owned@db.example/subtitle_test' },
    { MIGRATION_DATABASE_URL: 'postgres://board_public:owned@127.0.0.1/subtitle_test' },
    { MIGRATION_DATABASE_URL: valid.MIGRATION_DATABASE_URL + '?options=unsafe' },
    { MIGRATION_DATABASE_URL: '' }]) {
    assert.throws(() => subtitleDatabaseEnvironment({ ...valid, ...change }), /loopback development/);
  }
});


test('subtitle checks preserve source board headings in every mode, including archives and s4s', () => {
  for (const slug of ['owned', 's4s']) {
    const board = { slug, kind: 'none', title: 'Owned & title', description: 'Description is not a heading', textOnly: false };
    const prefix = slug === 's4s' ? '[s4s]' : `/${slug}/`;
    const html = heading => `<body><div class="boardBanner"><div class="boardTitle">${heading}</div></div></body>`;
    const expected = html(`${prefix} - Owned &amp; title`);
    for (const mode of ['index', 'thread', 'archived-thread', 'catalog', 'archive-index']) {
      assert.doesNotThrow(() => assertSubtitleDocument(expected, board, mode));
      assert.throws(() => assertSubtitleDocument(html(`${prefix} - Archive`), board, mode));
    }
    assert.throws(() => assertSubtitleDocument(expected, board, 'unknown'));
    if (slug === 's4s') assert.throws(() => assertSubtitleDocument(html('/s4s/ - Owned &amp; title'), board, 'archive-index'));
  }
});
