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

test('GIF presentation uses approved URL format while filename and thumbnail remain independent', () => {
  const gif = original.replaceAll('/demo/1.png', '/demo/1.gif').replace('2 KB PNG', '2 KB GIF');
  assert.equal(parse(gif).status, 'ok');
  assert.equal(parse(gif.replace('2 KB GIF', '2 KB PNG')).status, 'invalid-snapshot');
  assert.equal(parse(original.replace('2 KB PNG', '2 KB GIF')).status, 'invalid-snapshot');
  assert.equal(parse(gif.replaceAll('/demo/1.gif', '/demo/1.GIF')).status, 'invalid-snapshot');
  assert.equal(parse(gif.replace('/demo/1s.jpg', '/demo/2s.jpg')).status, 'invalid-snapshot');
  assert.equal(parse(gif.replace('/demo/1s.jpg', '/demo/1.gif')).status, 'ok');
});

test('fixed spoiler and deleted assets require their complete original file recipe', () => {
  const wrap = file => original.slice(0, original.indexOf('<div id="f1"')) + file + original.slice(original.indexOf('<blockquote'));
  const spoiler = '<div class="file" id="f1"><div class="fileText" id="fT1" title="Owned.png">File: <a href="https://media.test/demo/1.png" rel="noopener noreferrer">Spoiler Image</a> (2 KB, 600x360)</div><a class="fileThumb imgspoiler" href="https://media.test/demo/1.png" rel="noopener noreferrer"><img src="/static/catalog/spoiler.png" alt="2 KB" width="100" height="100" loading="lazy"><div class="mFileInfo mobile">2 KB PNG</div></a></div>';
  const deleted = '<div class="file" id="f1"><span class="fileThumb"><img class="fileDeletedRes" src="/static/catalog/filedeleted-res.gif" srcset="/static/catalog/filedeleted-res@2x.gif 2x" alt="File deleted." width="127" height="13" loading="lazy"></span></div>';
  for (const file of [spoiler, deleted]) assert.equal(parse(wrap(file)).status, 'ok');
  for (const changed of [
    spoiler.replace('title="Owned.png"', ''), spoiler.replace('width="100"', 'width="101"'),
    spoiler.replace('alt="2 KB"', 'alt="Owned.png"'), spoiler.replace('/static/catalog/spoiler.png', 'https://media.test/demo/1s.jpg'),
    spoiler.replace('class="fileThumb imgspoiler"', 'class="fileThumb"'),
    spoiler.replace('class="file" id="f1"', 'class="file" id="f1" data-image-spoiler="true" data-image-filename="Another.png"'),
    deleted.replace('/static/catalog/filedeleted-res@2x.gif 2x', 'https://other.test/file.gif 2x'),
    deleted.replace('class="fileDeletedRes"', 'class="fileDeletedRes op"'), deleted.replace('width="127"', 'width="128"'),
    deleted.replace('File deleted.', 'Owned.png'), deleted.replace('class="fileThumb"', 'class="postNum"'),
    '<img src="/static/catalog/spoiler.png" alt="2 KB" width="100" height="100">',
  ]) assert.equal(parse(wrap(changed)).status, 'invalid-snapshot', changed);
});

test('PDF captions identify a document and only its static preview can load as an image', () => {
  const pdf = original.replaceAll('/demo/1.png', '/demo/1.pdf')
    .replace('2 KB PNG', '2 KB PDF').replace('600x360', 'PDF');
  assert.equal(parse(pdf).status, 'ok');
  for (const changed of [pdf.replace(', PDF)', ', 1x1)'),
    pdf.replace('2 KB PDF', '2 KB PNG'),
    pdf.replace('/demo/1s.jpg', '/demo/1.pdf'),
    pdf.replaceAll('/demo/1.pdf', '/demo/1.PDF'),
    original.replace('600x360', 'PDF')]) {
    assert.equal(parse(changed).status, 'invalid-snapshot');
  }
});

test('video caption formats bind static thumbnails without permitting media as image sources', () => {
  for (const ext of ['webm', 'mp4']) {
    const video = original.replaceAll('/demo/1.png', `/demo/1.${ext}`).replace('2 KB PNG', `2 KB ${ext.toUpperCase()}`);
    assert.equal(parse(video).status, 'ok');
    for (const changed of [video.replace('/demo/1s.jpg', `/demo/1.${ext}`),
      video.replace(`2 KB ${ext.toUpperCase()}`, '2 KB PNG'),
      video.replaceAll(`/demo/1.${ext}`, `/demo/1.${ext.toUpperCase()}`)]) {
      assert.equal(parse(changed).status, 'invalid-snapshot');
    }
  }
});

test('JPEG and mobile markers survive native projection only on matching file thumbnails', () => {
  const jpeg = original.replaceAll('/demo/1.png', '/demo/1.jpg').replace('2 KB PNG', '2 KB JPG');
  const mobile = jpeg.replace('class="fileThumb"', 'class="fileThumb" data-m');
  assert.equal(parse(jpeg).status, 'ok');
  assert.equal(parse(mobile).status, 'ok');
  assert.equal(parse(original.replace('class="fileThumb"', 'class="fileThumb" data-m')).status, 'ok');
  for (const changed of [
    mobile.replace(' data-m', ' data-m="true"'),
    jpeg.replace('File: <a ', 'File: <a data-m '),
    mobile.replaceAll('/demo/1.jpg', '/demo/1.gif').replace('2 KB JPG', '2 KB GIF'),
    mobile.replaceAll('/demo/1.jpg', '/demo/1m.jpg'),
    mobile.replace('src="https://media.test/demo/1s.jpg"', 'src="https://media.test/demo/1m.jpg"'),
  ]) assert.equal(parse(changed).status, 'invalid-snapshot', changed);
});


test('canonical display filename permits an appended extension without expanding raw intake', () => {
  function withFilename(name) {
    return original.replace('>Owned.png</a>', ` title="${name}">${fileLabel(name, true)}</a>`)
      .replace('alt="Owned.png"', `alt="${name}"`);
  }
  for (const name of ['a'.repeat(255) + '.png', 'é'.repeat(127) + 'a.png']) {
    assert.equal(new TextEncoder().encode(name).length, 259);
    assert.equal(parse(withFilename(name)).status, 'ok');
    assert.equal(parse(withFilename('a' + name)).status, 'invalid-snapshot');
  }
});
