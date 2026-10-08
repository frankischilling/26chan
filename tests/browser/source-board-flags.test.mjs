import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { boardFlagCodes } from '../../apps/public/client/native-board-flag-codes.js';
import { isPostFlagClass, isPostFlagToken } from '../../apps/public/client/native-post-flags.js';
import { parseUpdaterSnapshot } from '../../apps/public/client/native-updater-snapshot.js';
import { validatePostTree } from '../../apps/public/static/native-filter.v1.js';

const root = new URL('../../', import.meta.url);
const reference = JSON.parse(await readFile(new URL('apps/public/tests/fixtures/board-flags.json', root)));
const assets = JSON.parse(await readFile(new URL('docs/source-board-flag-assets.json', root)));
const context = { origin: 'https://flags.example', board: 'demo', thread: '1000', mediaOrigin: '' };
export function flagSnapshot(kind, code, title) {
  const css = `bfl bfl-${code.toLowerCase()}${kind === 'pol' ? '' : ` bfl-type-${kind}`}`;
  const html = `<article class="postContainer opContainer" id="pc1000"><div class="post op" id="p1000"><div class="postInfo" id="pi1000"><span class="name">Anonymous</span><span class="${css}" title="${title}"></span><span class="postNum"><a href="/demo/thread/1000#p1000" title="Link to this post">No.</a><a href="/demo/thread/1000?quote=1000#reply" title="Reply to this post">1000</a></span></div><blockquote class="postMessage" id="m1000">Owned flag</blockquote></div></article>`;
  return { version: 2, tail_size: 0, tail_id: null, board: 'demo', thread: '1000', closed: false, archived: false,
    sticky: false, replies: 0, images: 0, posts: [{ no: '1000', file_deleted: false, html }] };
}

test('all 165 source definitions retain finite type membership through source and released validators', () => {
  assert.equal(Object.values(reference.tables).reduce((sum, table) => sum + Object.keys(table.display).length, 0), 165);
  for (const [kind, table] of Object.entries(reference.tables)) {
    assert.deepEqual(boardFlagCodes[kind].trim().split(' '), Object.keys(table.display).map(code => code.toLowerCase()));
    for (const [code, title] of Object.entries(table.display)) {
      const css = `bfl bfl-${code.toLowerCase()}${kind === 'pol' ? '' : ` bfl-type-${kind}`}`;
      assert.ok(isPostFlagClass(css), css);
      assert.ok(css.split(' ').every(isPostFlagToken));
      const parsed = parseUpdaterSnapshot(JSON.stringify(flagSnapshot(kind, code, title)), context);
      assert.equal(parsed.status, 'ok', `${kind}/${code}`);
      assert.doesNotThrow(() => validatePostTree(parsed.snapshot.posts[0].tree, context, '1000'));
    }
  }
  assert.equal(reference.tables.pol.selector.BL, 'Black Nationalist');
  assert.equal(reference.tables.pol.display.BL, 'Black Lives Matter');
  assert.equal(reference.tables.mlp.display.AN, 'Anon');
  assert.equal(reference.tables.pol.display.AN, 'Anarchist');
  assert.deepEqual(reference.boards.filter(board => board.enabled).map(board => board.board).sort(), ['mlp', 'pol']);
});
test('unknown or mixed namespaces, active attributes and malformed flag titles fail before DOM construction', () => {
  const original = flagSnapshot('mlp', 'TWI', 'Twilight Sparkle');
  for (const invalid of ['bfl bfl-ac bfl-type-mlp', 'bfl bfl-twi', 'bfl bfl-twi bfl-type-pol',
    'bfl bfl-twi bfl-type-unknown', 'bfl bfl-twi bfl-type-mlp bfl-type-lgbt', 'bfl bfl-TWI bfl-type-mlp']) {
    assert.equal(isPostFlagClass(invalid), false, invalid);
    const candidate = structuredClone(original);
    candidate.posts[0].html = candidate.posts[0].html.replace('bfl bfl-twi bfl-type-mlp', invalid);
    assert.equal(parseUpdaterSnapshot(JSON.stringify(candidate), context).status, 'invalid-snapshot');
  }
  for (const title of ['" onclick="alert(1)', '&#10;control', 'é'.repeat(51)]) {
    assert.equal(parseUpdaterSnapshot(JSON.stringify(flagSnapshot('mlp', 'TWI', title)), context).status, 'invalid-snapshot');
  }
});
test('retrieved sprites retain their hashes and every source code has a scoped CSS rule', async () => {
  const css = await readFile(new URL('apps/public/static/flags/board-types.css', root), 'utf8');
  assert.ok(!css.includes('s.4cdn.org'));
  assert.ok(!/(?:@import|https?:|data:)/.test(css));
  for (const kind of ['pol', 'mlp', 'lgbt']) {
    const row = assets[kind].find(row => row.url.includes('.png'));
    const file = kind === 'pol' ? 'board-flags.2.png' : `${kind}-flags.${kind === 'mlp' ? 3 : 1}.png`;
    const bytes = await readFile(new URL(`apps/public/static/flags/${file}`, root));
    assert.equal(createHash('sha256').update(bytes).digest('hex'), row.sha256);
    assert.equal(bytes.length, row.bytes);
    assert.equal(bytes.subarray(1, 4).toString(), 'PNG');
    const rules = kind === 'pol' ? await readFile(new URL('apps/public/static/flags/flags.css', root), 'utf8') : css;
    for (const code of Object.keys(reference.tables[kind].display)) {
      assert.ok(rules.includes(`${kind === 'pol' ? '' : `.bfl-type-${kind}`}.bfl-${code.toLowerCase()} {`), `${kind}/${code}`);
    }
  }
  assert.equal(assets.test[0].status, 404);
});
