import test from 'node:test';
import assert from 'node:assert/strict';
import { selectionIds } from '../../apps/public/static/source-post-actions.v1.js';
import { parsePostRecipe, UPDATER_LIMITS } from '../../apps/public/client/native-updater-snapshot.js';
import { prepareQuotePost } from '../../apps/public/client/native-quote-preview.js';

const no = '9223372036854775807';
const context = { origin: 'https://boards.test', board: 'demo', thread: '9007199254740993', mediaOrigin: '' };
const control = `<input type="checkbox" id="select${no}" name="${no}" value="delete" form="delform" aria-label="Select post No.${no}">`;
const html = (header = control, body = 'Owned text') => `<article class="postContainer replyContainer" id="pc${no}"><div class="post reply" id="p${no}"><div class="postInfo" id="pi${no}">${header}</div><blockquote class="postMessage" id="m${no}">${body}</blockquote></div></article>`;
const parse = value => parsePostRecipe(value, context, no, { nodes: 0 }, UPDATER_LIMITS);

test('selection preserves decimal identity and native submitted order', () => {
  const input = [no, '2', '9007199254740993'];
  assert.deepEqual(selectionIds(input), input);
  assert.notEqual(selectionIds(input), input);
  assert.deepEqual(selectionIds(['1'], true), ['1']);
});

test('field count includes mode, password and the optional file-only control', () => {
  const ids = Array.from({ length: 19 }, (_, index) => String(index + 1));
  assert.equal(selectionIds(ids.slice(0, 18)).length, 18);
  assert.equal(selectionIds(ids.slice(0, 17), true).length, 17);
  assert.throws(() => selectionIds(ids), /at most 18/);
  assert.throws(() => selectionIds(ids.slice(0, 18), true), /at most 17/);
  assert.equal(selectionIds(ids.slice(0, 17), false, true).length, 17);
  assert.equal(selectionIds(ids.slice(0, 16), true, true).length, 16);
  assert.throws(() => selectionIds(ids.slice(0, 18), false, true), /at most 17/);
  assert.throws(() => selectionIds(ids.slice(0, 17), true, true), /at most 16/);
  assert.throws(() => selectionIds(['1'], false, 'true'));
});

test('empty, duplicate, coerced and noncanonical selections fail before submission', () => {
  for (const ids of [[], ['1', '1'], [1], ['01'], ['0'], ['-1'], ['1\n'], ['１'], ['9223372036854775808'], ['1e2'], null]) {
    assert.throws(() => selectionIds(ids));
  }
  assert.throws(() => selectionIds(['1'], 'true'));
});

test('worker accepts only the exact unchecked selector in its own desktop header', () => {
  const tree = parse(html());
  const input = tree.children[0].children[0].children[0];
  assert.equal(input.attrs.name, no);
  assert.equal(input.attrs.form, 'delform');
  for (const [from, to] of [
    ['type="checkbox"', 'type="hidden"'], ['value="delete"', 'value="report"'],
    ['form="delform"', 'form="reply"'], [`name="${no}"`, 'name="2"'],
    [`id="select${no}"`, 'id="select2"'], [`No.${no}`, 'No.2'],
    ['<input ', '<input checked '], ['<input ', '<input disabled '],
    ['<input ', '<input formaction="/demo/delete" '],
    ['<input ', '<input formtarget="_blank" '],
  ]) assert.throws(() => parse(html(control.replace(from, to))), `${from} -> ${to}`);
  assert.throws(() => parse(html(control + control)));
  assert.throws(() => parse(html(`<span>${control}</span>`)));
  assert.throws(() => parse(html('', control)));
  assert.throws(() => parse(html('', `<form method="post" action="/demo/delete">${control}</form>`)));
  assert.throws(() => parse(html(`<a href="/demo/">${control}</a>`)));
  assert.throws(() => parse(html().replace('class="postInfo"', 'class="postInfoM mobile"')));
  assert.doesNotThrow(() => parse(html(''))); // Older inert fixtures and local previews have no selector.
});

test('quote copies discard the admitted selector instead of duplicating form ownership', () => {
  const plan = prepareQuotePost(parse(html()), context, no);
  const document = {
    createTextNode: text => ({ text }),
    createElement: tag => ({ tag, attrs: {}, children: [], setAttribute(key, value) { this.attrs[key] = value; }, append(node) { this.children.push(node); } }),
    cookie: '', querySelector: () => null,
  };
  const result = plan.build(document);
  const serialized = JSON.stringify(result);
  assert.ok(!serialized.includes('delform') && !serialized.includes('select' + no));
  assert.ok(!serialized.includes('"tag":"input"'));
  assert.ok(serialized.includes('Owned text'));
});
