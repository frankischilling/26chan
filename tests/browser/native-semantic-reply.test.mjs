import test from 'node:test';
import assert from 'node:assert/strict';
import { parseUpdaterSnapshot, parseQuotePreviewSnapshot, parseBoardPageSnapshot,
  postLinkUrl, updaterContext, validatePostTree, validateBoardPageSnapshot } from '../../apps/public/client/native-updater-snapshot.js';
import { validatePostTree as validateReleasedPostTree } from '../../apps/public/static/native-filter.v1.js';
import { parseSearchPayload } from '../../apps/public/client/global-search.js';

const input = { origin: 'https://board.example', board: 'demo', thread: '9007199254740993' };
const path = context => `/${context.board}/thread/${context.thread}`;
function post(context, href, label = 'Reply') {
  const no = context.thread;
  return { no, file_deleted: false, html: `<article class="postContainer opContainer" id="pc${no}"><div class="post op" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span><span class="postNum"><a href="${path(context)}#p${no}" title="Link to this post">No.</a><a href="${path(context)}?quote=${no}#reply" title="Reply to this post">${no}</a></span> [<a href="${href}">${label}</a>]</div><blockquote class="postMessage" id="m${no}">Body</blockquote></div></article>` };
}
function updater(context, value) {
  return { version: 2, board: context.board, thread: context.thread, closed: false, archived: false,
    sticky: false, replies: 0, images: 0, posts: [value], tail_size: 0, tail_id: null };
}
function page(context, value) {
  return { version: 2, board: context.board, page: 0, next_page: null, replies_shown: 3, threads: [{ thread: context.thread,
    closed: false, sticky: false, archived: false, replies: 0, images: 0, omitted: 0, posts: [value] }] };
}
function replyLink(tree) {
  return tree.children[0].children[0].children.find(node => node?.tag === 'a');
}
function search(context, value) {
  return JSON.stringify({ threads: [{ board: context.board, thread: context.thread,
    posts: [{ no: value.no, html: value.html }] }], offset: 0, nhits: 1 });
}

test('ordinary semantic OP links survive all shared parser and main-thread revalidation consumers', () => {
  for (const thread of ['1', '9007199254740992', '9007199254740993', '9223372036854775807']) {
    const context = updaterContext({ ...input, thread });
    for (const suffix of ['', '/subject-with-123', '/' + 'a'.repeat(49)]) for (const label of ['Reply', 'View thread']) {
      const href = path(context) + suffix, value = post(context, href, label);
      const parsed = parseUpdaterSnapshot(JSON.stringify(updater(context, value)), context);
      assert.equal(parsed.status, 'ok', href);
      const tree = parsed.snapshot.posts[0].tree;
      assert.equal(replyLink(tree).attrs.href, href);
      assert.deepEqual(replyLink(tree).attrs, { href }); // No class/action authority added.
      assert.equal(parsed.snapshot.posts[0].no, thread);
      assert.doesNotThrow(() => validatePostTree(tree, context, thread));
      assert.doesNotThrow(() => validateReleasedPostTree(tree, context, thread));
      assert.equal(parseQuotePreviewSnapshot(JSON.stringify({ version: 1, board: context.board,
        thread, post: value }), { ...context, post: thread }).status, 'ok');
      const boardPage = parseBoardPageSnapshot(JSON.stringify(page(context, value)), { ...context, page: 0 });
      assert.equal(boardPage.status, 'ok');
      assert.doesNotThrow(() => validateBoardPageSnapshot(boardPage.snapshot, { ...context, page: 0 }));
      const result = parseSearchPayload(search(context, value), context);
      assert.equal(replyLink(result.threads[0].posts[0].tree).attrs.href, href);
    }
  }
});

test('semantic OP paths reject encoded, malformed, cross-origin and wrong-identity targets', () => {
  const base = path(input), accepted = `${base}/subject`;
  const bad = [
    ...['', '-word', 'word-', 'word--word', 'Upper', 'under_score', 'has.dot', '%61', '%2F', '%3Cscript%3E',
      'one/two', 'a'.repeat(50), 'subject/', 'subject?x=1', 'subject#reply', 'subject\n', 'subject\r',
      'subject\t', 'subject\\extra', 'subject\u0000', 'café'].map(suffix => `${base}/${suffix}`),
    `/other/thread/${input.thread}/subject`, `/Demo/thread/${input.thread}/subject`,
    ...['0', '-1', '+1', '01', '9007199254740992', '9007199254740994', '9223372036854775808',
      '99999999999999999999', '1.json', '1-tail.json', '%39' + input.thread.slice(1)].map(id => `/demo/thread/${id}/subject`),
    `/%64emo/thread/${input.thread}/subject`, `${base}/../subject`, `${base}//subject`,
    `//evil.example${accepted}`, `https://evil.example${accepted}`, `https://board.example${accepted}`,
  ];
  const tree = parseUpdaterSnapshot(JSON.stringify(updater(input, post(input, accepted))), input).snapshot.posts[0].tree;
  for (const href of bad) {
    // HTTP comment links keep their existing separate rel policy; OP links
    // have only an href, so absolute URLs cannot cross this renderer recipe.
    if (href.startsWith('/')) assert.equal(postLinkUrl(href, input), false, href);
    const altered = structuredClone(tree);
    replyLink(altered).attrs.href = href;
    assert.throws(() => validatePostTree(altered, input, input.thread), undefined, href);
    assert.throws(() => validateReleasedPostTree(altered, input, input.thread), undefined, href);
    assert.equal(parseUpdaterSnapshot(JSON.stringify(updater(input, post(input, href))), input).status, 'invalid-snapshot', href);
    assert.equal(parseQuotePreviewSnapshot(JSON.stringify({ version: 1, board: input.board,
      thread: input.thread, post: post(input, href) }), { ...input, post: input.thread }).status, 'invalid-preview', href);
    assert.equal(parseBoardPageSnapshot(JSON.stringify(page(input, post(input, href))), { ...input, page: 0 }).status, 'invalid-snapshot', href);
    assert.throws(() => parseSearchPayload(search(input, post(input, href)), input), undefined, href);
  }
  for (const thread of ['0', '01', '9223372036854775808']) {
    assert.equal(postLinkUrl(`/demo/thread/${thread}/subject`, { ...input, thread }), false);
  }
  const changed = structuredClone(tree);
  replyLink(changed).attrs.class = 'replylink';
  assert.throws(() => validatePostTree(changed, input, input.thread));
});
