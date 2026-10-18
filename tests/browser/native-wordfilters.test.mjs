import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { parseUpdaterSnapshot, parseQuotePreviewSnapshot } from '../../apps/public/client/native-updater-snapshot.js';
import { isWordfilterMarkup } from '../../apps/public/client/native-wordfilter-markup.js';

const source = JSON.parse(readFileSync(new URL('../../fixtures/wordfilter-posting-reference.json', import.meta.url)));
const context = { origin: 'https://board.example', board: 'g', thread: '100' };
const article = inside => '<article class="postContainer opContainer" id="pc100"><div class="post op" id="p100">'
  + '<div class="postInfo" id="pi100"><span class="name">Anonymous</span><span class="postNum">'
  + '<a href="/g/thread/100#p100" title="Link to this post">No.</a>'
  + '<a href="/g/thread/100?quote=100#reply" title="Reply to this post">100</a></span></div>'
  + `<blockquote class="postMessage" id="m100">${inside}</blockquote></div></article>`;
function updater(html) {
  return parseUpdaterSnapshot(JSON.stringify({ version: 2, board: 'g', thread: '100', closed: false,
    archived: false, sticky: false, replies: 0, images: 0, tail_size: 0, tail_id: null,
    posts: [{ no: '100', file_deleted: false, html }] }), context);
}
function preview(html) {
  return parseQuotePreviewSnapshot(JSON.stringify({ version: 1, board: 'g', thread: '100',
    post: { no: '100', file_deleted: false, html } }), { ...context, post: '100' });
}

test('independent PHP Test markup survives both finite worker recipes for all saved choices', () => {
  const cases = source.profiles.test.filter(case_ => /\[(?:code|spoiler|sjis)\]/.test(case_.input));
  assert.equal(cases.length, 432);
  for (const case_ of cases) {
    // HTML5 ignores these digit-tag closing comments. The Rust template omits
    // them while escaping their literal openings, preserving visible content.
    const html = article(case_.final.replace(/<\/5(?:pan|p4n)?>/g, ''));
    assert.equal(updater(html).status, 'ok', `${case_.input} ${case_.rolls}`);
    assert.equal(preview(html).status, 'ok', `${case_.input} ${case_.rolls}`);
  }
});

test('filtered recipes cannot add attributes, classes, resource loads or header authority', () => {
  const positive = '<pr3 cl4ss="pr3ttyprint">safe</pr3><sp4n cl4ss="mu-r">safe</sp4n>';
  assert.equal(updater(article(positive)).status, 'ok');
  for (const hostile of [
    '<pr3 cl4ss="pr3ttyprint" onclick="bad()">unsafe</pr3>',
    '<pr3 cl4ss="pr3ttyprint" id="p100">unsafe</pr3>',
    '<pr3 class="prettyprint">impossible saved combination</pr3>',
    '<sp4n cl4ss="arbitrary">unsafe</sp4n>',
    '<sp4n cl4ss="mu-r" src="https://tracker.example/pixel">unsafe</sp4n>',
    '<script>bad()</script>', '<iframe src="https://tracker.example/"></iframe>',
    '<span cl4ss="mu-r" style="color:red">unsafe</span>',
  ]) {
    assert.equal(updater(article(hostile)).status, 'invalid-snapshot', hostile);
    assert.equal(preview(article(hostile)).status, 'invalid-preview', hostile);
  }
  const moved = article('safe').replace('<span class="name">Anonymous</span>', positive);
  assert.equal(updater(moved).status, 'invalid-snapshot');
  assert.equal(preview(moved).status, 'invalid-preview');
  assert.equal(isWordfilterMarkup('pr3', { cl4ss: 'pr3ttyprint', onload: 'bad()' }), false);
  assert.equal(isWordfilterMarkup('pr3', { cl4ss: ['pr3ttyprint'] }), false);
});

test('generated fortune recipes preserve finite CSS colors for every saved Test choice', () => {
  const colors = ['7fec11', 'bac200', 'e7890c', 'fd4d32', 'f51c6a', 'd302a7', '9d05da', '6023f8', '2a56fb', '0893e1', '00cbb0', '16f174', '43fd3b'];
  const css = readFileSync(new URL('../../assets/comment-markup.css', import.meta.url), 'utf8');
  const change = (text, choices) => {
    for (const choice of new Set(choices)) {
      text = choice === 5 ? text.replace(/([^gl])[tT]/g, '$17')
        : text.replace([/a/gi, /e/gi, /i/gi, /o/gi, /s/gi][choice], ['4', '3', '1', '0', '5'][choice]);
    }
    return text;
  };
  for (let first = 0; first < 6; first++) for (let second = 0; second < 6; second++) {
    const choices = [first, second], tag = change('span', choices);
    for (let index = 0; index < colors.length; index++) {
      const attribute = change('class', choices);
      const attrs = { [attribute]: change('fortune', choices) };
      if (choices.includes(4)) {
        assert.equal(isWordfilterMarkup(tag, attrs), false);
        continue;
      }
      if (choices.every(choice => choice === 0 || choice === 2)) {
        const color = change(colors[index], choices);
        const class_ = `fortune-${color === colors[index] ? '' : 'a-'}${index}`;
        assert.ok(css.includes(`.${class_} { color: #${color}; }`));
        if (attribute === 'class') attrs.class += ` ${class_}`;
        else attrs.class = class_;
      }
      assert.equal(isWordfilterMarkup(tag, attrs), true, JSON.stringify({ choices, index, tag, attrs }));
      const html = article(`<${tag}${Object.entries(attrs).map(([key, value]) => ` ${key}="${value}"`).join('')}>fortune</${tag}>`);
      assert.equal(updater(html).status, 'ok');
      assert.equal(preview(html).status, 'ok');
      assert.equal(isWordfilterMarkup(tag, { ...attrs, style: `color:#${colors[index]}` }), false);
      assert.equal(isWordfilterMarkup(tag, { ...attrs, id: 'm100' }), false);
    }
  }
  for (const inside of ['<sp4n cl4ss="fortune" class="fortune-a-2">unknown palette</sp4n>',
    '<sp4n class="fortune-a-1">missing source attributes</sp4n>',
    '<span class="fortune fortune-a-1">wrong source element</span>',
    '<sp4n cl4ss="fortune" class="fortune-a-1" style="color:#b4c200">inline style</sp4n>']) {
    assert.equal(updater(article(inside)).status, 'invalid-snapshot');
    assert.equal(preview(article(inside)).status, 'invalid-preview');
  }
});
