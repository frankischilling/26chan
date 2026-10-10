import test from 'node:test';
import assert from 'node:assert/strict';
import { imageTarget, imageSize, IMAGE_LIMITS } from '../../apps/public/client/native-images.js';
import { parseUpdaterSnapshot } from '../../apps/public/client/native-updater-snapshot.js';

test('images require exact approved PNG or GIF routes on the configured media origin', () => {
  const origin = 'https://media.example';
  for (const path of ['/a/1.png', '/a/1.gif', '/demo/9007199254740993.gif', '/demo/9007199254740993.png', '/0123456789/9223372036854775807.png']) {
    assert.deepEqual(imageTarget(origin + path, origin), { url: origin + path, thumbnail: origin + path.slice(0, -4) + 's.jpg' });
  }
  for (const value of [null, 1, '/a/1.png', '//media.example/a/1.png', 'https://other.example/a/1.png',
    `${origin}.attacker.example/a/1.png`, `${origin}@attacker.example/a/1.png`, `${origin}/a/0.png`,
    `${origin}/a/01.png`, `${origin}/a/9223372036854775808.png`, `${origin}/a/1.png?x`, `${origin}/a/1.png#x`,
    `${origin}/a/1.png\n`, `${origin}/a/1.png\u0000`, `${origin}/a\\1.png`, `${origin}/a/../a/1.png`,
    `${origin}/a/%31.png`, `${origin}/a/1s.jpg`, `${origin}/a/1.svg`, `${origin}/a/1.webm`,
    `${origin}/a/1.jpg`, `${origin}/A/1.png`, `${origin}/abcdefghijk/1.png`, 'data:image/png,bytes']) {
    assert.equal(imageTarget(value, origin), null, String(value));
  }
  for (const path of ['/a/1.GIF', '/a/1.gif?x', '/a/1.gif#x', '/a/01.gif', '/a/0.gif', '/a/1.thumb.gif', '/a/1.gif.png', '/a/9223372036854775808.gif']) {
    assert.equal(imageTarget(origin + path, origin), null, path);
  }
  for (const origin of ['', 'file://media.example', 'https://user@media.example', 'https://media.example/',
    'https://media.example/path', 'https://media.example:443', 'https://media.example?x']) {
    assert.equal(imageTarget(`${origin}/a/1.png`, origin), null);
  }
  assert.ok(imageTarget('http://localhost:3004/img/1.png', 'http://localhost:3004'));
});

test('image fitting preserves the aspect ratio and never enlarges small images', () => {
  for (const [input, expected] of [
    [[600, 360, 1000], { width: 600, height: 360 }],
    [[600, 360, 300], { width: 300, height: 180 }],
    [[240, 600, 300, 300], { width: 120, height: 300 }],
    [[600, 360, 100, 50], { width: 600 * (50 / 360), height: 50 }],
    [[48, 32, 1000, 900], { width: 48, height: 32 }],
  ]) assert.deepEqual(imageSize(...input), expected);
  for (const input of [[0, 1, 10], [1, 0, 10], [1, 1, 0], [NaN, 1, 10], [1, Infinity, 10], [1, 1, 10, 0]]) {
    assert.equal(imageSize(...input), null);
  }
});

test('updater accepts bounded spoiler dimensions and rejects metadata that can bypass the file boundary', () => {
  const context = { origin: 'https://boards.example', mediaOrigin: 'https://media.example', board: 'img', thread: '1' };
  const parse = attrs => parseUpdaterSnapshot(JSON.stringify({
    version: 2, board: 'img', thread: '1', closed: false, archived: false, sticky: false,
    replies: 0, images: 0, tail_size: 0, tail_id: null,
    posts: [{ no: '1', file_deleted: false, html: `<article class="postContainer opContainer" id="pc1"><div class="post op" id="p1"><div class="postInfo" id="pi1"><span class="name">Anonymous</span></div><div ${attrs}><p>File: <a href="https://media.example/img/1.png" target="_blank" rel="noopener noreferrer">Owned.png</a></p><details><summary>Spoiler image</summary><a href="https://media.example/img/1.png" rel="noopener noreferrer">View spoiler image</a></details></div><blockquote class="postMessage" id="m1">Owned</blockquote></div></article>` }],
  }), context);
  const valid = 'class="file" id="f1" data-image-spoiler="true" data-image-filename="Owned.png" data-thumbnail-width="250" data-thumbnail-height="150"';
  assert.equal(parse(valid).status, 'ok');
  assert.equal(parse(valid + ' data-thumbnail-legacy="true"').status, 'ok');
  assert.equal(parse(valid.replace('Owned.png', 'é'.repeat(127) + 'x')).status, 'ok');
  for (const attrs of [valid.replace('250', '0'), valid.replace('250', '1025'), valid.replace('250', '01'),
    valid.replace('250', '250px'), valid.replace('class="file"', 'class="postInfo"'),
    valid.replace('data-image-spoiler="true"', 'data-image-spoiler="false"'), valid + ' data-thumbnail-src="https://tracker.invalid/1.png"',
    valid.replace('Owned.png', ''), valid.replace('Owned.png', 'é'.repeat(128)), valid.replace('Owned.png', 'bad&#13;name.png'),
    valid.replace('Owned.png', 'bad&#x81;name.png')]) {
    assert.equal(parse(attrs).status, 'invalid-snapshot', attrs);
  }
  assert.equal(IMAGE_LIMITS.dimension, 1024);
});
