import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareQuotePost } from '../../apps/public/client/native-quote-preview.js';
import { parseQuotePreviewSnapshot, PREVIEW_LIMITS } from '../../apps/public/client/native-updater-snapshot.js';
import { INLINE_LIMITS, mountNativeInlineQuotes } from '../../apps/public/client/native-inline-quotes.js';
import { createCommentProjection } from '../../apps/public/client/native-comment-projection.js';

const context = { origin: 'https://board.example', mediaOrigin: 'https://media.example', board: 'demo', thread: '100' };
const html = `<article class="postContainer replyContainer" id="pc101"><div class="post reply" id="p101"><div class="postInfo" id="pi101"><span class="name">Anonymous</span></div><blockquote class="postMessage" id="m101"><s>masked</s><a class="quotelink" href="/demo/thread/100#p102">&gt;&gt;102</a></blockquote><details class="postActions"><summary>Delete</summary><form method="post" action="/demo/delete"><input type="hidden" name="no" value="101"><input type="password" name="password" id="delete101" autocomplete="off" required><button>Delete</button></form></details></div></article>`;
const parsed = () => parseQuotePreviewSnapshot(JSON.stringify({ version: 1, board: 'demo', thread: '100',
  post: { no: '101', file_deleted: false, html } }), { ...context, post: '101' });

test('prepared inline recipes charge the exact inert subtree before construction', () => {
  const tree = parsed().snapshot.post.tree;
  const plan = prepareQuotePost(tree, context, '101');
  let built = 0;
  const document = {
    createElement(tag) { built++; return { tag, attrs: {}, children: [],
      setAttribute(name, value) { this.attrs[name] = value; }, append(child) { this.children.push(child); } }; },
    createTextNode(data) { built++; return { data }; },
  };
  assert.equal(built, 0);
  const copy = plan.build(document);
  assert.equal(plan.nodes, built);
  assert.deepEqual(plan.quotes, ['/demo/thread/100#p102']);
  const serialized = JSON.stringify(copy);
  assert.doesNotMatch(serialized, /"id"|password|form|input|button|details|postActions/);
  assert.match(serialized, /"s"/);
  assert.doesNotMatch(serialized, /reveal-spoilers/);
  let characters = 0;
  function count(node) {
    if ('data' in node) { characters += node.data.length; return; }
    for (const [key, value] of Object.entries(node.attrs)) characters += key.length + value.length;
    node.children.forEach(count);
  }
  count(copy);
  assert.equal(plan.characters, characters);
});

test('forged worker recipes fail validation before an inert build plan is returned', () => {
  for (const mutate of [
    tree => { tree.children[0].children[1].children.push({ tag: 'video', attrs: { src: 'https://tracker.example/a' }, children: [] }); },
    tree => { tree.children[0].children[1].children[1].attrs.href = 'https://name:password@tracker.example/'; },
    tree => { tree.children[0].children[1].children.push('x'.repeat(PREVIEW_LIMITS.bytes)); },
    tree => { tree.children[0].attrs.id = 'p102'; },
  ]) {
    const tree = structuredClone(parsed().snapshot.post.tree); mutate(tree);
    assert.throws(() => prepareQuotePost(tree, context, '101'));
  }
});

test('projection authority uses exact objects and survives queued removal records', () => {
  const projection = createCommentProjection(), metadata = { closed: false };
  const owned = { nodeType: 1, parentNode: null }, forged = { nodeType: 1, className: 'preview inlined', parentNode: null };
  const child = { parentNode: owned };
  projection.claim(owned, metadata);
  assert.equal(projection.owner(child), metadata);
  assert.equal(projection.within(forged), false);
  assert.throws(() => projection.claim(owned, {}));
  metadata.closed = true;
  assert.equal(projection.originalMutation({ type: 'childList', target: forged, addedNodes: [], removedNodes: [owned] }), false);
  assert.equal(projection.originalMutation({ type: 'childList', target: forged, addedNodes: [], removedNodes: [forged] }), true);
  assert.equal(createCommentProjection().within(owned), false);
});

test('inline admission has finite aggregate, pending and nesting ceilings', () => {
  assert.deepEqual(INLINE_LIMITS, { open: 16, pending: 8, depth: 8, nodes: 32768,
    characters: 524288, quotes: 512, pendingMs: 15000 });
  assert.equal(mountNativeInlineQuotes(), null);
  assert.equal(mountNativeInlineQuotes({ root: { matches: () => false } }), null);
});
