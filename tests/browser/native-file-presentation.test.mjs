import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileLabel, validateFilePresentation } from '../../apps/public/client/native-file-presentation.js';
import { parseUpdaterSnapshot } from '../../apps/public/client/native-updater-snapshot.js';

const reference = JSON.parse(await readFile(new URL('../../docs/public-file-reference.json', import.meta.url)));
test('filename labels match the pinned formatter including split UTF-16 and inert entity text', () => {
  for (const row of reference.cases) assert.equal(fileLabel(`${row.filename}.png`, row.kind === 'op'), row.label);
});
const context = { origin: 'https://board.test', board: 'demo', thread: '1', mediaOrigin: 'https://media.test' };
const original = `<article id="pc1" class="postContainer opContainer"><div id="p1" class="post op"><div id="pi1" class="postInfo"><span class="name">Owned</span></div><div id="f1" class="file"><div id="fT1" class="fileText">File: <a href="https://media.test/demo/1.png" target="_blank" rel="noopener noreferrer">Owned.png</a> (2 KB, 600x360)</div><a class="fileThumb" href="https://media.test/demo/1.png" target="_blank" rel="noopener noreferrer"><img src="https://media.test/demo/1s.jpg" alt="Owned.png" width="250" height="150" loading="lazy"><div class="mFileInfo mobile">2 KB PNG</div></a></div><blockquote id="m1" class="postMessage">Owned</blockquote></div></article>`;
function parse(html) {
  return parseUpdaterSnapshot(JSON.stringify({ version: 2, board: 'demo', thread: '1', closed: false, archived: false,
    sticky: false, replies: 0, images: 0, posts: [{ no: '1', file_deleted: false, html }], tail_size: 0, tail_id: null }), context);
}
test('real updater grammar binds file header, caption, original filename and media targets', () => {
  assert.equal(parse(original).status, 'ok');
  for (const changed of [
    original.replace('id="fT1"', 'id="fT2"'), original.replace('class="fileText"', 'class="fileText mobile"'),
    original.replace('2 KB PNG', '3 KB PNG'), original.replace('2 KB PNG', '<a href="/demo/post/2">2 KB PNG</a>'),
    original.replace('class="mFileInfo mobile"', 'class="mFileInfo"'),
    original.replace('alt="Owned.png"', 'alt="Another.png"'),
    original.replace('>Owned.png</a>', ' title="Forged.png">Owned.png</a>'),
    original.replace('File: <a', 'File: <br><a'), original.replace('600x360', '600×360'),
    original.replace('>Owned.png</a>', '><s>Owned.png</s></a>'),
    original.replace('class="mFileInfo mobile"', 'class="mFileInfo mobile" data-tip-cb="unexpected"'),
    original.replace('width="250"', 'width="251"'),
    original.replaceAll('href="https://media.test/demo/1.png"', 'href="https://other.test/demo/1.png"'),
    original.replace('src="https://media.test/demo/1s.jpg"', 'src="https://media.test/demo/2s.jpg"'),
    original.replace('class="fileText"', 'class="fileText op"').replace('<div class="mFileInfo mobile">2 KB PNG</div>', ''),
  ]) assert.equal(parse(changed).status, 'invalid-snapshot', changed);
  const parsed = parse(original).snapshot.posts[0].tree;
  assert.doesNotThrow(() => validateFilePresentation(parsed, context, '1'));
});
