import test from 'node:test';
import assert from 'node:assert/strict';
import { nativeCommentText } from '../../apps/public/client/native-filter-html.js';
import { FILTER_LIMITS } from '../../apps/public/client/native-filter-limits.js';

test('native comment conversion retains exact br spelling and the pinned character-class behavior', () => {
  assert.equal(nativeCommentText('a<br>b<BR>c<br/>d<br >e'), 'a\nbcde');
  assert.equal(nativeCommentText('a>>b[>c^>d<>e'), 'abcde');
});

test('native HTML fragment parsing retains active-document noscript text without executing code', () => {
  assert.equal(nativeCommentText('<noscript><b>paper</b> &amp;</noscript>'), '<b>paper</b> &amp;');
  assert.equal(nativeCommentText('<script>globalThis.ownedHtmlExecuted=true;</script>'), 'globalThis.ownedHtmlExecuted=true;');
  assert.equal(globalThis.ownedHtmlExecuted, undefined);
});

test('native comment entities decode once, including numeric replacements and astral text', () => {
  assert.equal(nativeCommentText('&lt;b&gt;literal&lt;/b&gt; &amp; &#0; &#x1f642;'), '<b>literal</b> & \ufffd \ud83d\ude42');
  assert.equal(nativeCommentText('&amp;lt;b&amp;gt;'), '&lt;b&gt;');
  assert.equal(nativeCommentText('<span>&gt;&gt;123</span><wbr> &amp; tail'), '>>123 & tail');
});

test('native div-context parsing preserves foster-parenting order and excludes template content', () => {
  assert.equal(nativeCommentText('<table>before<tr><td>cell</td></tr>after</table>'), 'beforeaftercell');
  assert.equal(nativeCommentText('<tr><td>A</td></tr>B'), 'AB');
  assert.equal(nativeCommentText('<template>hidden</template>shown'), 'shown');
  assert.equal(nativeCommentText('x<!-- hidden -->y'), 'xy');
});

test('empty markup and parser newline normalization retain their native text values', () => {
  assert.equal(nativeCommentText('<span></span>'), '');
  assert.equal(nativeCommentText(''), '');
  assert.equal(nativeCommentText('a\r\nb\rc'), 'a\nb\nc');
  assert.equal(nativeCommentText('<textarea>&amp; &lt;b&gt;</textarea>'), '& <b>');
});

test('HTML fields and decoded text remain bounded without truncating filter inputs', () => {
  assert.equal(nativeCommentText('x'.repeat(FILTER_LIMITS.field)).length, FILTER_LIMITS.field);
  assert.throws(() => nativeCommentText('x'.repeat(FILTER_LIMITS.field + 1)), /text-limit/);
  assert.throws(() => nativeCommentText('x'.repeat(FILTER_LIMITS.html + 1)), /html-limit/);
  assert.throws(() => nativeCommentText(null), /html-limit/);
});

test('resource-bearing markup remains plain parser data without browser globals', () => {
  assert.equal(typeof document, 'undefined');
  const raw = '<img src="https://owned.invalid/image" onerror="ownedHandler()">'
    + '<iframe src="https://owned.invalid/frame">frame text</iframe>'
    + '<style>@import url(https://owned.invalid/style);</style>'
    + '<owned-element>custom text</owned-element>';
  assert.equal(nativeCommentText(raw), 'frame text@import url(https://owned.invalid/style);custom text');
});
