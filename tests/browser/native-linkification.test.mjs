import test from 'node:test';
import assert from 'node:assert/strict';
import { hasSourceLink, sourceLinkSpans, linkificationEnabled, LINKIFY_LIMITS } from '../../apps/public/client/native-linkification.js';

const labels = input => sourceLinkSpans(input).map(({ start, end }) => input.slice(start, end));

test('source lexical boundaries, global probe and original-parenthesis rule', () => {
  for (const [input, expected] of [
    ['https://example.test/path', ['https://example.test/path']],
    ['https://example.test/path?!', ['https://example.test/path']],
    ['(https://example.test/a(b))', ['https://example.test/a(b)']],
    ['(https://example.test/a(b)).', ['https://example.test/a(b))']],
    ['HTTPS://EXAMPLE.TEST/path', []],
    ['https://lower.test HTTPS://UPPER.TEST/path', ['https://lower.test', 'HTTPS://UPPER.TEST/path']],
    ['xhttps://example.test/path', ['https://example.test/path']],
    ['Bhttps://example.test/path', []],
    ['"https://example.test/path"', []],
    ['https://example.test:8443/path', ['https://example.test']],
    ['http://127.0.0.1/path', []],
    ['https://user:pass@example.test/path', []],
    ['<a href="https://example.test/path">https://example.test/path</a>', []],
    ['<s>https://example.test/path</s>', ['https://example.test/path']],
  ]) assert.deepEqual(labels(input), expected, input);
});

test('source entity spelling and soft breaks survive labels but not URL breaks', () => {
  const input = 'https://example.test/abcdefgh\u200bijk?a=1&amp;b=2';
  const [span] = sourceLinkSpans(input);
  assert.equal(input.slice(span.start, span.end), input);
  assert.equal(decodeURIComponent(span.parameter), input.replace('\u200b', ''));
  assert.equal(hasSourceLink('https://exam\u200bple.test'), false);
});

test('source option precedence uses mobile layout and exact storage string', () => {
  assert.equal(linkificationEnabled({}, false, null), false);
  assert.equal(linkificationEnabled({ linkify: true }, false, null), true);
  assert.equal(linkificationEnabled({ linkify: false }, true, null), true);
  assert.equal(linkificationEnabled({ linkify: false }, true, 'true'), false);
  assert.equal(linkificationEnabled({ linkify: false }, true, 'false'), true);
  assert.equal(linkificationEnabled({ linkify: true, disableAll: true }, true, null), false);
});

test('bounded malformed input and unsafe destination exceptions stay plain', () => {
  assert.deepEqual(sourceLinkSpans('https://example.test/' + 'x'.repeat(LINKIFY_LIMITS.characters)), []);
  for (const input of ['https://example.test/\u0000', 'https://example.test/\\host', 'https://example.test/\ud800']) {
    assert.deepEqual(sourceLinkSpans(input), []);
  }
  for (let count = 0; count < 128; count++) {
    const input = 'https://example.test/' + '('.repeat(count) + 'x' + ')'.repeat(count + 2) + '?!';
    for (const span of sourceLinkSpans(input)) {
      assert.ok(span.start >= 0 && span.end <= input.length && span.start < span.end);
      const url = new URL(decodeURIComponent(span.parameter));
      assert.ok(['http:', 'https:'].includes(url.protocol));
      assert.equal(url.username + url.password, '');
    }
  }
});
