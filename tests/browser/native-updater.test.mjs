import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { useUpdaterTail } from '../../apps/public/client/native-updater-tail.js';
import { parseUpdaterSnapshot, parseQuotePreviewSnapshot, parseBoardPageSnapshot, validatePostTree, updaterUrl, UPDATER_LIMITS } from '../../apps/public/client/native-updater-snapshot.js';
import { NativeUpdaterTransport } from '../../apps/public/client/native-updater-transport.js';
import { mobileHeaderLabel } from '../../apps/public/client/native-post-numbers.js';

const context = { origin: 'https://board.example', board: 'demo', thread: '9007199254740992', mediaOrigin: 'https://media.example' };
const html = (no, inside = 'Safe &lt;script&gt; &amp; text') => `<article class="postContainer ${no === context.thread ? 'opContainer' : 'replyContainer'}" id="pc${no}"><div class="post ${no === context.thread ? 'op' : 'reply'}" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span><span class="postNum"><a href="/demo/thread/${context.thread}#p${no}" title="Link to this post">No.</a><a href="/demo/thread/${context.thread}?quote=${no}#reply" title="Reply to this post">${no}</a></span></div><blockquote class="postMessage" id="m${no}">${inside}</blockquote><details class="postActions"><summary>Delete or report</summary><form method="post" action="/demo/delete"><input type="hidden" name="no" value="${no}"><label for="delete${no}">Deletion password</label><input id="delete${no}" name="password" type="password" minlength="8" maxlength="128" autocomplete="off" required><button>Delete post</button></form></details></div></article>`;
function snapshot(inside) {
  return { version: 2, tail_size: 0, tail_id: null, board: 'demo', thread: context.thread, closed: false, archived: false, sticky: false,
    replies: 1, images: 0, posts: [context.thread, '9007199254740993'].map(no => ({ no, file_deleted: false, html: html(no, inside) })) };
}
function parse(s, c = context) { return parseUpdaterSnapshot(JSON.stringify(s), c); }

const drawingAnnotations = JSON.parse(readFileSync(new URL('../../crates/domain/tests/fixtures/drawing-annotation/cases.json', import.meta.url)));
const inertAnnotations = drawingAnnotations.time.filter(case_ => case_.expected_text.startsWith('<br><br><small>')
  && !case_.expected_text.includes('Replay:'));

function commentNode(tree, no) {
  function find(node) {
    if (typeof node === 'string') return null;
    if (node.tag === 'blockquote' && node.attrs.class === 'postMessage' && node.attrs.id === `m${no}`) return node;
    return node.children.map(find).find(Boolean) ?? null;
  }
  return find(tree);
}
function commentText(node) {
  return typeof node === 'string' ? node : node.children.map(commentText).join('');
}
function annotationPage(posts) {
  return { version: 2, board: context.board, page: 0, next_page: null, replies_shown: 3,
    threads: [{ thread: context.thread, closed: false, sticky: false, archived: false,
      replies: 1, images: 0, omitted: 0, posts }] };
}

test('source drawing annotation survives updater, preview, board page and live validation as inert comment markup', () => {
  assert.ok(inertAnnotations.some(case_ => case_.id === 'source'));
  assert.ok(inertAnnotations.some(case_ => case_.id === 'source_suppresses_replay'));
  assert.ok(inertAnnotations.length > 10);
  for (const { id, expected_text } of inertAnnotations) {
    const value = snapshot(`Saved drawing${expected_text}`), no = value.posts[1].no;
    const updater = parse(value);
    assert.equal(updater.status, 'ok', `updater: ${id}`);
    const preview = parseQuotePreviewSnapshot(JSON.stringify({ version: 1, board: context.board,
      thread: context.thread, post: value.posts[1] }), { ...context, post: no });
    assert.equal(preview.status, 'ok', `preview: ${id}`);
    const board = parseBoardPageSnapshot(JSON.stringify(annotationPage(value.posts)), { ...context, page: 0 });
    assert.equal(board.status, 'ok', `board page: ${id}`);
    const expectedText = `Saved drawing${expected_text.replace(/<[^>]+>/g, '').replaceAll('&gt;', '>')}`;
    for (const tree of [updater.snapshot.posts[1].tree, preview.snapshot.post.tree,
      board.snapshot.threads[0].posts[1].tree]) {
      assert.doesNotThrow(() => validatePostTree(tree, context, no), `live tree: ${id}`);
      const comment = commentNode(tree, no);
      assert.ok(comment, id);
      assert.equal(commentText(comment), expectedText, `decoded text: ${id}`);
      const small = comment.children.find(child => typeof child !== 'string' && child.tag === 'small');
      assert.ok(small, `small retained: ${id}`);
      assert.deepEqual(small.attrs, {}, `small attributes: ${id}`);
      assert.deepEqual(small.children[0], { tag: 'b', attrs: {}, children: ['Oekaki Post'] }, `heading: ${id}`);
    }
  }
});

test('drawing annotation grants no attributes, active content, resources or controls and never escapes comments', () => {
  const safe = drawingAnnotations.time.find(case_ => case_.id === 'source').expected_text;
  assert.equal(parse(snapshot(safe)).status, 'ok');
  const badComments = [
    safe.replace('<small>', '<small onclick="alert(1)">'),
    safe.replace('<small>', '<small style="display:none">'),
    safe.replace('<small>', '<small class="quote">'),
    safe.replace('<small>', '<small id="pi9007199254740993">'),
    safe.replace('<small>', '<small src="https://tracker.example/pixel.png">'),
    safe.replace('<small>', '<small data-annotation="owned">'),
    safe.replace('<b>', '<b onmouseover="alert(1)">'),
    safe.replace('</small>', '<script>alert(1)</script></small>'),
    safe.replace('</small>', '<style>body{display:none}</style></small>'),
    safe.replace('</small>', '<img src="https://tracker.example/pixel.png" alt="tracking"></small>'),
    safe.replace('</small>', '<input type="hidden" name="password" value="secret"></small>'),
    safe.replace('</small>', '<button>Submit</button></small>'),
    `<a class="quotelink" href="/demo/post/42">${safe}</a>`,
  ];
  for (const inside of badComments) {
    const value = snapshot(inside), no = value.posts[1].no;
    assert.equal(parse(value).status, 'invalid-snapshot', `updater: ${inside}`);
    assert.equal(parseQuotePreviewSnapshot(JSON.stringify({ version: 1, board: context.board,
      thread: context.thread, post: value.posts[1] }), { ...context, post: no }).status,
    'invalid-preview', `preview: ${inside}`);
    assert.equal(parseBoardPageSnapshot(JSON.stringify(annotationPage(value.posts)), { ...context, page: 0 }).status,
    'invalid-snapshot', `board page: ${inside}`);
  }
  const outside = snapshot(safe);
  outside.posts[1].html = outside.posts[1].html.replace('<span class="name">Anonymous</span>',
    '<small><b>Oekaki Post</b></small><span class="name">Anonymous</span>');
  assert.equal(parse(outside).status, 'invalid-snapshot', 'small in post header');
  assert.equal(parseQuotePreviewSnapshot(JSON.stringify({ version: 1, board: context.board,
    thread: context.thread, post: outside.posts[1] }), { ...context, post: outside.posts[1].no }).status,
  'invalid-preview', 'preview with small outside comment');
  assert.equal(parseBoardPageSnapshot(JSON.stringify(annotationPage(outside.posts)), { ...context, page: 0 }).status,
  'invalid-snapshot', 'board page with small outside comment');

  const parsed = parse(snapshot(safe));
  assert.equal(parsed.status, 'ok');
  const no = parsed.snapshot.posts[1].no;
  const injected = structuredClone(parsed.snapshot.posts[1].tree);
  commentNode(injected, no).children.find(child => typeof child !== 'string' && child.tag === 'small').attrs.onclick = 'alert(1)';
  assert.throws(() => validatePostTree(injected, context, no), /invalid-snapshot/, 'main-thread revalidation');
  const moved = structuredClone(parsed.snapshot.posts[1].tree);
  const body = moved.children.find(node => typeof node !== 'string' && node.attrs.id === `p${no}`);
  const comment = commentNode(moved, no);
  const at = comment.children.findIndex(child => typeof child !== 'string' && child.tag === 'small');
  body.children.unshift(comment.children.splice(at, 1)[0]);
  assert.throws(() => validatePostTree(moved, context, no), /invalid-snapshot/, 'small outside comment');
});

test('automatic deletion fields remain empty and bound to their own post and action', () => {
  const positive = snapshot(), no = positive.posts[1].no;
  for (const post of positive.posts) {
    post.html = post.html.replace(`<label for="delete${post.no}">Deletion password</label>`, '')
      .replace('type="password" minlength="8" maxlength="128" autocomplete="off" required', 'type="hidden"');
  }
  assert.equal(parse(positive).status, 'ok');
  for (const [from, to] of [
    [`id="delete${no}"`, `id="delete${context.thread}"`],
    ['name="password" type="hidden"', 'name="password" type="hidden" value=""'],
    ['name="password" type="hidden"', 'name="password" type="hidden" value="private-capability"'],
    ['name="password" type="hidden"', 'name="anonymous_capability" type="hidden"'],
    ['name="password" type="hidden"', 'name="password" type="text"'],
    ['action="/demo/delete"', 'action="/demo/report"'],
  ]) {
    const candidate = structuredClone(positive);
    candidate.posts[1].html = candidate.posts[1].html.replace(from, to);
    assert.notEqual(candidate.posts[1].html, positive.posts[1].html);
    assert.equal(parse(candidate).status, 'invalid-snapshot', to);
  }
});

test('source board, catalog search and rules links survive the bounded recipe without opening other routes', () => {
  for (const href of ['/po/', '/g/catalog', '/g/catalog#s=a+b%2Fc%2Cd-e', '/rules#g3', '/rules#unknown4']) {
    const inside = `<a class="quotelink" href="${href}">&gt;&gt;&gt;/g/catalog</a>`;
    assert.equal(parse(snapshot(inside)).status, 'ok', href);
  }
  for (const href of ['//evil.example/g/catalog', '/g/catalog?admin=1', '/g/catalog#s=%2F..%2F',
    '/g/catalog#s=a%20b', '/g/catalog#s=a%2fb', '/rules#../admin', '/g/../admin', '/admin',
    '/g/catalog#s=a b', 'javascript:alert(1)']) {
    const inside = `<a class="quotelink" href="${href}">owned</a>`;
    assert.equal(parse(snapshot(inside)).status, 'invalid-snapshot', href);
  }
});

test('fortune palette classes and dice markup survive the finite updater recipe without inline style authority', () => {
  const inside = '<b>Rolled 6, 6 = 12 (2d6)<br><br></b>ordinary'
    + '<span class="fortune fortune-10"><br><br><b>Your fortune: Outlook good</b></span>';
  assert.equal(parse(snapshot(inside)).status, 'ok');
  for (const hostile of [
    inside.replace('fortune-10', 'fortune-owned'),
    inside.replace('class="fortune fortune-10"', 'class="fortune fortune-10" style="color:#00cbb0"'),
    inside.replace('fortune fortune-10', 'fortune fortune-10 quote'),
  ]) assert.equal(parse(snapshot(hostile)).status, 'invalid-snapshot', hostile);
});

const capcodes = [
  ['Mod', 'capcodeMod', 'id_mod', 'Highlight posts by Moderators', 'modicon', 'This user is a board Moderator.'],
  ['Admin', 'capcodeAdmin', 'id_admin', 'Highlight posts by Administrators', 'adminicon', 'This user is a board Administrator.'],
  ['Manager', 'capcodeManager', 'id_manager', 'Highlight posts by Managers', 'managericon', 'This user is a board Manager.'],
  ['Developer', 'capcodeDeveloper', 'id_developer', 'Highlight posts by Developers', 'developericon', 'This user is a board Developer.'],
  ['Founder', 'capcodeAdmin', 'id_admin', 'Highlight posts by the Founder', 'foundericon', "This user is the board's Founder."],
];
function badgeHtml([label, nameClass, group, title, icon, iconTitle]) {
  return `<span class="nameBlock ${nameClass}"><span class="name">Owned &lt;staff&gt;</span> <strong class="capcode hand ${group}" title="${title}">## ${label}</strong> <img class="identityIcon" src="/static/identity/${icon}.gif"${icon === 'foundericon' ? '' : ` srcset="/static/identity/${icon}@2x.gif 2x"`} alt="${iconTitle}" title="${iconTitle}" width="16" height="16"></span>`;
}
function capcodeSnapshot(def = capcodes[0], highlighted = false) {
  const value = snapshot();
  value.posts[1].html = value.posts[1].html.replace('<span class="name">Anonymous</span>', badgeHtml(def));
  if (highlighted) value.posts[1].html = value.posts[1].html.replace('class="post reply"', 'class="post reply highlightPost"');
  return value;
}

test('staff badges admit only complete pinned header recipes and fixed icon fetches', () => {
  for (const def of capcodes) assert.equal(parse(capcodeSnapshot(def)).status, 'ok', def[0]);
  assert.equal(parse(capcodeSnapshot(capcodes[1], true)).status, 'ok');
  for (const def of [capcodes[0], ...capcodes.slice(2)]) {
    assert.equal(parse(capcodeSnapshot(def, true)).status, 'invalid-snapshot');
  }
  const positive = capcodeSnapshot();
  for (const [from, to] of [
    ['## Mod', '## Admin'], ['capcodeMod', 'capcodeAdmin'], ['id_mod', 'id_admin'],
    ['Highlight posts by Moderators', 'arbitrary title'], ['width="16"', 'width="32"'],
    ['/static/identity/modicon.gif', '/static/identity/unlisted.gif'],
    ['/static/identity/modicon.gif', 'https://board.example/static/identity/modicon.gif'],
    ['/static/identity/modicon@2x.gif 2x', 'https://tracker.example/pixel.gif 2x'],
    ['/static/identity/modicon@2x.gif 2x', '/static/identity/adminicon@2x.gif 2x'],
    ['class="identityIcon"', 'class="identityIcon" onload="bad()"'],
    ['class="identityIcon"', 'class="identityIcon" loading="lazy"'],
    ['This user is a board Moderator.', 'This user is an Administrator.'],
    ['</strong>', '<span>extra</span></strong>'],
    ['</span><span class="postNum"', '<span class="posteruid">forged ID</span></span><span class="postNum"'],
  ]) {
    const value = structuredClone(positive); value.posts[1].html = value.posts[1].html.replace(from, to);
    assert.notEqual(value.posts[1].html, positive.posts[1].html, `Mutation must apply: ${from}`);
    assert.equal(parse(value).status, 'invalid-snapshot', to);
  }
  for (const inside of [badgeHtml(capcodes[0]), '<strong class="capcode hand id_mod" title="Highlight posts by Moderators">## Mod</strong>',
    '<img src="/static/identity/modicon.gif" alt="icon">',
    '<img src="https://media.example/demo/123s.jpg" alt="media" title="arbitrary">',
    '<img src="https://media.example/demo/123s.jpg" alt="media" srcset="https://media.example/demo/124s.jpg 2x">']) {
    assert.equal(parse(snapshot(inside)).status, 'invalid-snapshot', inside);
  }
  const duplicated = capcodeSnapshot();
  duplicated.posts[1].html = duplicated.posts[1].html.replace(badgeHtml(capcodes[0]), badgeHtml(capcodes[0]).repeat(2));
  assert.equal(parse(duplicated).status, 'invalid-snapshot');
});

test('staff badges retain only prepared trip hashes in their own bounded name block', () => {
  for (const def of capcodes) for (const trip of ['!ozOtJW9BFA', '!!ABCDEFGHIJK']) {
    const positive = capcodeSnapshot(def);
    const hash = `<span class="postertrip">${trip}</span>`;
    positive.posts[1].html = positive.posts[1].html.replace('</span> <strong', `</span> ${hash} <strong`);
    assert.equal(parse(positive).status, 'ok', `${def[0]} ${trip}`);
    for (const [from, to] of [
      [hash, `<span class="postertrip">${trip}x</span>`],
      [hash, '<span class="postertrip">private-password</span>'],
      [hash, `<span class="postertrip" title="extra">${trip}</span>`],
      [hash, `<span class="postertrip"><span>${trip}</span></span>`],
      [hash, `${hash} ${hash}`],
      ['</blockquote>', `${hash}</blockquote>`],
    ]) {
      const candidate = structuredClone(positive);
      candidate.posts[1].html = candidate.posts[1].html.replace(from, to);
      assert.notEqual(candidate.posts[1].html, positive.posts[1].html);
      assert.equal(parse(candidate).status, 'invalid-snapshot', to);
    }
    const moved = structuredClone(positive);
    moved.posts[1].html = moved.posts[1].html.replace(` ${hash}`, '').replace('</blockquote>', `${hash}</blockquote>`);
    assert.equal(parse(moved).status, 'invalid-snapshot');
    const limit = 255 - new TextEncoder().encode(`</span> <span class="postertrip">${trip}`).length;
    for (const [name, expected] of [['n'.repeat(limit), 'ok'], ['n'.repeat(limit + 1), 'invalid-snapshot'],
      ['&amp;'.repeat(Math.floor(limit / 5)), 'ok'], ['&amp;'.repeat(Math.floor(limit / 5) + 1), 'invalid-snapshot']]) {
      const candidate = structuredClone(positive);
      candidate.posts[1].html = candidate.posts[1].html.replace('Owned &lt;staff&gt;', name);
      assert.equal(parse(candidate).status, expected, `${trip}: ${name.length}`);
    }
  }
});

test('post numbers bind both links, labels and titles to their own post header', () => {
  const positive = snapshot(), no = positive.posts[1].no;
  const permalink = `/demo/thread/${context.thread}#p${no}`;
  const reply = `/demo/thread/${context.thread}?quote=${no}#reply`;
  assert.equal(parse(positive).status, 'ok');
  assert.equal(parse(snapshot('<a href="https://example.org/?quote=ordinary" rel="nofollow noreferrer noopener">ordinary URL</a>')).status, 'ok');
  const closed = structuredClone(positive);
  closed.posts[1].html = closed.posts[1].html.replace(reply, permalink);
  assert.equal(parse(closed).status, 'ok');
  for (const [from, to] of [
    [permalink, `/other/thread/${context.thread}#p${no}`],
    [permalink, `/demo/thread/${no}#p${no}`],
    [reply, `/demo/thread/${context.thread}?quote=${context.thread}#reply`],
    [reply, `${reply}&extra=true`], [reply, `https://board.example${reply}`],
    ['title="Link to this post"', 'title="Reply to this post"'],
    ['title="Reply to this post"', 'title="arbitrary title"'],
    ['>No.</a>', '>No.0</a>'], [`>${no}</a>`, `><span>${no}</span></a>`],
    ['class="postNum"', 'class="postNum quote"'],
    ['title="Reply to this post"', 'title="Reply to this post" target="_blank"'],
  ]) {
    const value = structuredClone(positive);
    value.posts[1].html = value.posts[1].html.replace(from, to);
    assert.notEqual(value.posts[1].html, positive.posts[1].html, from);
    assert.equal(parse(value).status, 'invalid-snapshot', to);
  }
  const pair = positive.posts[1].html.match(/<span class="postNum">.*?<\/span>/)[0];
  for (const forged of [pair, `<a href="${reply}">reply</a>`,
    `<a href="${permalink}" title="Link to this post">link</a>`]) {
    assert.equal(parse(snapshot(forged)).status, 'invalid-snapshot');
  }
  const moved = structuredClone(positive);
  moved.posts[1].html = moved.posts[1].html.replace(pair, '').replace('</blockquote>', pair + '</blockquote>');
  assert.equal(parse(moved).status, 'invalid-snapshot');
});

function pairedHeaderSnapshot(def = null, highlighted = false) {
  const value = snapshot(), date = '09/08/26(Tue)08:00:00';
  for (const post of value.posts) {
    const op = post.no === context.thread, name = def ? badgeHtml(def) : '<span class="name">Anonymous</span>';
    const subject = '<span class="subject">Owned mobile subject</span>';
    const identity = def ? name.slice(0, -7) : `<span class="nameBlock">${name}`;
    const mobile = `<div class="postInfoM mobile" id="pim${post.no}">${identity}<br>${op ? subject + ' ' : ''}</span>`
      + `<span class="dateTime postNum" data-utc="1788868800">${date} <a href="/demo/thread/${context.thread}#p${post.no}" title="Link to this post">No.</a>`
      + `<a href="/demo/thread/${context.thread}?quote=${post.no}#reply" title="Reply to this post">${post.no}</a></span></div>`;
    post.html = post.html.replace('<span class="name">Anonymous</span>', `${op ? subject : ''}${name}<time datetime="2026-09-08T12:00:00Z">${date}</time>`)
      .replace(`<div class="postInfo" id="pi${post.no}">`, `${mobile}<div class="postInfo" id="pi${post.no}">`);
    if (highlighted) post.html = post.html.replace(`class="post ${op ? 'op' : 'reply'}"`, `class="post ${op ? 'op' : 'reply'} highlightPost"`);
  }
  return value;
}

test('paired staff trip headers survive updater, preview and board-page validation', () => {
  for (const def of capcodes) for (const trip of ['!ozOtJW9BFA', '!!ABCDEFGHIJK']) {
    const value = pairedHeaderSnapshot(def);
    for (const post of value.posts) post.html = post.html.replaceAll('</span> <strong',
      `</span> <span class="postertrip">${trip}</span> <strong`);
    const result = parse(value);
    assert.equal(result.status, 'ok');
    for (const post of result.snapshot.posts) assert.doesNotThrow(() => validatePostTree(post.tree, context, post.no));
    assert.equal(parseQuotePreviewSnapshot(JSON.stringify({ version: 1, board: context.board,
      thread: context.thread, post: value.posts[0] }), { ...context, post: context.thread }).status, 'ok');
    assert.equal(parseBoardPageSnapshot(JSON.stringify({ version: 2, board: context.board, page: 0, next_page: null, replies_shown: 3,
      threads: [{ thread: context.thread, closed: false, sticky: false, archived: false, replies: 1,
        images: 0, omitted: 0, posts: value.posts }] }), { ...context, page: 0 }).status, 'ok');
    const mismatched = structuredClone(value);
    mismatched.posts[1].html = mismatched.posts[1].html.replace(trip, trip === '!ozOtJW9BFA' ? '!0123456789' : '!!01234567890');
    assert.equal(parse(mismatched).status, 'invalid-snapshot');
  }
});

test('authorized names and subjects survive worker parsing and live tree validation within saved UTF-8 bounds', () => {
  const escape = text => text.replace(/[&<>"']/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#039;' })[character]);
  function candidate(name, subject) {
    const value = pairedHeaderSnapshot(capcodes[0]);
    const label = (text, css) => {
      const mobile = mobileHeaderLabel(text);
      return `<span class="${css}"${mobile.shortened ? ` title="${escape(text)}"` : ''}>${escape(mobile.text)}</span>`;
    };
    for (const post of value.posts) {
      post.html = post.html.replace('<span class="name">Owned &lt;staff&gt;</span>', label(name, 'name'))
        .replace('<span class="name">Owned &lt;staff&gt;</span>', `<span class="name">${escape(name)}</span>`);
      if (post.no === context.thread) post.html = post.html.replace('<span class="subject">Owned mobile subject</span>', label(subject, 'subject'))
        .replace('<span class="subject">Owned mobile subject</span>', `<span class="subject">${escape(subject)}</span>`);
    }
    return value;
  }
  for (const [name, subject] of [
    ['Owned <staff>' + 'n'.repeat(220), 's'.repeat(255)],
    ['n'.repeat(255), ' '.repeat(1014)],
    ['é'.repeat(127) + 'a', ' '.repeat(1020)],
    ['&'.repeat(51), 'é'.repeat(510)],
  ]) {
    const value = candidate(name, subject), result = parse(value);
    assert.equal(result.status, 'ok');
    for (const post of result.snapshot.posts) assert.doesNotThrow(() => validatePostTree(post.tree, context, post.no));
    const preview = { version: 1, board: context.board, thread: context.thread, post: value.posts[0] };
    assert.equal(parseQuotePreviewSnapshot(JSON.stringify(preview), { ...context, post: context.thread }).status, 'ok');
    const page = { version: 2, board: context.board, page: 0, next_page: null, replies_shown: 3, threads: [{
      thread: context.thread, closed: false, sticky: false, archived: false, replies: 1,
      images: 0, omitted: 0, posts: value.posts,
    }] };
    assert.equal(parseBoardPageSnapshot(JSON.stringify(page), { ...context, page: 0 }).status, 'ok');
  }
  for (const [name, subject] of [
    ['n'.repeat(256), 'owned'], ['é'.repeat(128), 'owned'],
    ['owned', ' '.repeat(1021)], ['owned', 'é'.repeat(511)],
  ]) assert.equal(parse(candidate(name, subject)).status, 'invalid-snapshot');
});

test('paired mobile headers bind identity, subject, timestamp and both number targets to the desktop recipe', () => {
  assert.equal(parse(pairedHeaderSnapshot()).status, 'ok');
  for (const def of capcodes) assert.equal(parse(pairedHeaderSnapshot(def)).status, 'ok', def[0]);
  assert.equal(parse(pairedHeaderSnapshot(capcodes[1], true)).status, 'ok');
  const positive = pairedHeaderSnapshot(), no = positive.posts[1].no;
  for (const [from, to] of [
    [`id="pim${no}"`, `id="pim${context.thread}"`], ['class="postInfoM mobile"', 'class="postInfoM"'],
    ['data-utc="1788868800"', 'data-utc="1788868801"'], ['data-utc="1788868800"', 'data-utc="01788868800"'],
    ['08:00:00 <a', '08:00:01 <a'], ['>Anonymous</span><br>', '>Another name</span><br>'],
    ['<br></span>', '<br><span class="subject">Forged reply subject</span></span>'],
    [`?quote=${no}#reply`, `?quote=${context.thread}#reply`],
    [`?quote=${no}#reply`, `#p${no}`],
    ['class="dateTime postNum"', 'class="dateTime postNum" title="arbitrary"'],
    ['class="nameBlock"', 'class="nameBlock capcodeAdmin"'],
  ]) {
    const value = structuredClone(positive);
    value.posts[1].html = value.posts[1].html.replace(from, to);
    assert.notEqual(value.posts[1].html, positive.posts[1].html, from);
    assert.equal(parse(value).status, 'invalid-snapshot', to);
  }
  const forged = structuredClone(positive);
  forged.posts[0].html = forged.posts[0].html.replace('Owned mobile subject', 'Mismatched mobile subject');
  assert.equal(parse(forged).status, 'invalid-snapshot');
  for (const inside of ['<span class="nameBlock">Outside header</span>',
    '<span class="dateTime" data-utc="1788868800">Unowned time</span>', '<span class="mobile">Hidden content</span>']) {
    const value = pairedHeaderSnapshot(); value.posts[1].html = value.posts[1].html.replace('</blockquote>', `${inside}</blockquote>`);
    assert.equal(parse(value).status, 'invalid-snapshot', inside);
  }
  const badge = pairedHeaderSnapshot(capcodes[0]);
  badge.posts[1].html = badge.posts[1].html.replace('## Mod', '## Admin');
  assert.equal(parse(badge).status, 'invalid-snapshot');
  const identity = pairedHeaderSnapshot();
  const suffix = ' <span class="postertrip">!OwnedTrip</span> <span class="posteruid">(ID: <span class="hand">AAAAAAAA</span>)</span>'
    + ' <span title="United Kingdom" class="flag flag-gb"></span>';
  for (const post of identity.posts) post.html = post.html.replaceAll('<span class="name">Anonymous</span>', '<span class="name">Anonymous</span>' + suffix);
  assert.equal(parse(identity).status, 'ok');
  for (const [from, to] of [['!OwnedTrip', '!ForeignTrip'], ['AAAAAAAA', 'BBBBBBBB'], ['flag-gb', 'flag-us'], ['United Kingdom', 'Foreign title']]) {
    const candidate = structuredClone(identity); candidate.posts[1].html = candidate.posts[1].html.replace(from, to);
    assert.equal(parse(candidate).status, 'invalid-snapshot', to);
  }
});

test('shortened mobile names and subjects retain only the exact escaped full-label title', () => {
  const escape = text => text.replace(/[&<>"']/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#039;' })[character]);
  for (const raw of ['A'.repeat(31), '<'.repeat(10), 'A'.repeat(29) + '😀', '&amp;lt;'.repeat(6), 'é'.repeat(50)]) {
    const value = pairedHeaderSnapshot(), label = mobileHeaderLabel(raw);
    assert.equal(label.shortened, true);
    const mobileName = `<span class="name" title="${escape(raw)}">${escape(label.text)}</span>`;
    const mobileSubject = `<span class="subject" title="${escape(raw)}">${escape(label.text)}</span>`;
    for (const post of value.posts) {
      post.html = post.html.replace('<span class="name">Anonymous</span>', mobileName)
        .replace('<span class="name">Anonymous</span>', `<span class="name">${escape(raw)}</span>`);
      if (post.no === context.thread) post.html = post.html.replace('<span class="subject">Owned mobile subject</span>', mobileSubject)
        .replace('<span class="subject">Owned mobile subject</span>', `<span class="subject">${escape(raw)}</span>`);
    }
    assert.equal(parse(value).status, 'ok', raw);
    for (const [from, to] of [
      [mobileName, mobileName.replace(`title="${escape(raw)}"`, 'title="forged label"')],
      [mobileName, mobileName.replace(` title="${escape(raw)}"`, '')],
      [mobileName, mobileName.replace(escape(label.text), 'forged text')],
      [mobileName, mobileName.replace('<span ', '<span onclick="bad()" ')],
      [mobileName, mobileName.replace('</span>', '<b>forged</b></span>')],
      [mobileSubject, mobileSubject.replace(`title="${escape(raw)}"`, 'title="forged subject"')],
    ]) {
      const candidate = structuredClone(value);
      candidate.posts[0].html = candidate.posts[0].html.replace(from, to);
      assert.notEqual(candidate.posts[0].html, value.posts[0].html, from);
      assert.equal(parse(candidate).status, 'invalid-snapshot', to);
    }
    const copied = structuredClone(value);
    copied.posts[1].html = copied.posts[1].html.replace('</blockquote>', mobileName + '</blockquote>');
    assert.equal(parse(copied).status, 'invalid-snapshot');
  }
  const short = pairedHeaderSnapshot();
  short.posts[1].html = short.posts[1].html.replace('<span class="name">Anonymous</span>', '<span class="name" title="Anonymous">Anonymous</span>');
  assert.equal(parse(short).status, 'invalid-snapshot');
});

test('country and board flags use finite inert classes and bounded titles', () => {
  for (const [name, css] of [['United Kingdom', 'flag flag-gb'], ['Unknown', 'flag flag-xx'],
    ['Anarcho-Capitalist', 'bfl bfl-ac'], ['United Nations', 'bfl bfl-un']]) {
    assert.equal(parse(snapshot(`<span title="${name}" class="${css}"></span>`)).status, 'ok');
  }
  for (const invalid of ['<span title="bad" class="flag flag-zz"></span>',
    '<span title="bad" class="bfl bfl-zz"></span>', '<span class="flag flag-gb"></span>',
    '<span title="bad" class="flag flag-gb">child</span>', '<span title="bad" class="quote"></span>',
    '<span title="bad" class="flag bfl-gb"></span>', '<div title="bad" class="flag flag-gb"></div>',
    '<span title="bad" class="flag flag-gb" style="display:none"></span>',
    `<span title="${'é'.repeat(51)}" class="flag flag-gb"></span>`,
    '<span title="bad&#10;title" class="flag flag-gb"></span>']) {
    assert.equal(parse(snapshot(invalid)).status, 'invalid-snapshot', invalid);
  }
});

test('owned rendering produces only data with exact adjacent large IDs and decoded text', () => {
  const result = parse(snapshot()); assert.equal(result.status, 'ok');
  assert.deepEqual(result.snapshot.posts.map(p => p.no), ['9007199254740992', '9007199254740993']);
  assert.ok(JSON.stringify(result).includes('Safe <script> & text'));
  assert.equal(updaterUrl(context), 'https://board.example/_watch/demo/thread/9007199254740992/posts');
  for (const bad of [{ thread: null }, { thread: '01' }, { thread: '9223372036854775808' }, { board: '../staff' },
    { origin: 'https://u:p@board.example' }, { origin: 'https://board.example/path' }, { mediaOrigin: 'javascript:alert(1)' }]) {
    assert.throws(() => updaterUrl({ ...context, ...bad }));
  }
});

test('active content, foreign namespaces, unapproved fetches and credential-bearing forms are rejected', () => {
  for (const content of ['<script>alert(1)</script>', '<svg><a href="/">bad</a></svg>', '<style>body{display:none}</style>',
    '<img src="https://tracker.example/a.png" alt="bad">', '<iframe src="/staff"></iframe>', '<a href="javascript:alert(1)">bad</a>',
    '<a href="//tracker.example/">bad</a>', '<a href="https://u:p@host.example/" rel="noopener noreferrer">bad</a>',
    '<span onclick="alert(1)">bad</span>', '<span style="color:red">bad</span>', '<div id="pi123">collision</div>',
    '<template><img src="/tracker"></template>']) assert.equal(parse(snapshot(content)).status, 'invalid-snapshot', content);
  for (const [from, to] of [['/demo/delete', '/staff/remove'], ['value="9007199254740993"', 'value="1"'],
    ['type="password"', 'type="password" value="leaked"'], ['<form method="post"', '<form target="_blank" method="post"'],
    ['class="postMessage"', 'class="arbitraryClass"']]) {
    const s = snapshot(); s.posts[1].html = s.posts[1].html.replace(from, to);
    assert.equal(parse(s).status, 'invalid-snapshot', to);
  }
});

test('approved formatting and normalized media are allowed without accepting unrelated URLs', () => {
  assert.equal(parse(snapshot('<span class="quote">&gt;long<wbr>word</span><a href="https://example.org/long" rel="nofollow noreferrer noopener">long<wbr>link</a>')).status, 'ok');
  for (const invalid of ['<wbr onclick="bad()">', '<wbr class="quote">', '<wbr style="color:red">']) {
    assert.equal(parse(snapshot(invalid)).status, 'invalid-snapshot');
  }
  for (const color of ['mu-s', 'mu-i', 'mu-r', 'mu-g', 'mu-b']) {
    assert.equal(parse(snapshot(`<span class="${color}">&lt;script&gt;literal&lt;/script&gt;</span>`)).status, 'ok');
    assert.equal(parse(snapshot(`<div class="${color}">wrong element</div>`)).status, 'invalid-snapshot');
    assert.equal(parse(snapshot(`<span class="${color} quote">mixed</span>`)).status, 'invalid-snapshot');
  }
  const content = '<span class="quote">&gt;green</span><br><span class="spoiler" tabindex="0" aria-label="Spoiler; focus to reveal">secret</span>'
    + '<a class="quotelink" href="/other/post/42">&gt;&gt;&gt;/other/42</a>'
    + '<a href="https://example.org/path?q=test" rel="nofollow noreferrer noopener">link</a>'
    + '<a class="fileThumb" href="https://media.example/demo/123.png" target="_blank" rel="noopener noreferrer"><img src="https://media.example/demo/123s.jpg" alt="file" width="250" height="100" loading="lazy"></a>';
  assert.equal(parse(snapshot(content)).status, 'ok');
  assert.equal(parse(snapshot(content), { ...context, mediaOrigin: '' }).status, 'invalid-snapshot');
  assert.equal(parse(snapshot(content.replace('/123s.jpg', '/../private.png'))).status, 'invalid-snapshot');
  const longUrl = `https://example.org/${encodeURIComponent('折'.repeat(4000))}`;
  assert.equal(parse(snapshot(`<a href="${longUrl}" rel="nofollow noreferrer noopener">${longUrl}</a>`)).status, 'ok');
});

test('source multiline markup crosses only the finite inert snapshot grammar', () => {
  for (const content of [
    '<s>first<br><s>&lt;script&gt;second</s></s>',
    '<pre class="prettyprint">first<br>second</pre>',
    '<span class="sjis">a  b<br>c</span>',
    '<s>first<pre class="prettyprint">crossed</s>tail</pre>',
    '<s><a class="quotelink" href="/demo/post/42">&gt;&gt;42</a></s>',
  ]) {
    const result = parse(snapshot(content));
    assert.equal(result.status, 'ok', content);
    assert.ok(JSON.stringify(result.snapshot.posts[0].tree).length > 0);
  }
  for (const content of [
    '<s onclick="bad()">bad</s>', '<s style="display:none">bad</s>',
    '<pre>bad</pre>', '<pre class="quote">bad</pre>',
    '<pre class="prettyprint" src="https://tracker.example/">bad</pre>',
    '<span class="sjis" onmouseover="bad()">bad</span>',
    '<pre class="prettyprint"><script>bad()</script></pre>',
  ]) assert.equal(parse(snapshot(content)).status, 'invalid-snapshot', content);
});

test('partial, oversized, duplicate, unordered and mismatched snapshots fail as a whole', () => {
  for (const edit of [s => s.posts.pop(), s => s.posts.reverse(), s => s.posts[1].no = s.posts[0].no,
    s => s.board = 'other', s => s.closed = 1, s => s.version = 99, s => s.images = 2,
    s => s.extra = 1, s => s.posts[1].html = s.posts[1].html.replace('id="pi9007199254740993"', 'id="p9007199254740993"'),
    s => s.posts[1].html += '<p>extra root</p>', s => s.posts = Array(1002).fill(s.posts[0])]) {
    const s = snapshot(); edit(s); assert.equal(parse(s).status, 'invalid-snapshot');
  }
  assert.equal(parseUpdaterSnapshot('{', context).status, 'invalid-snapshot');
  assert.equal(parse(snapshot('x'.repeat(UPDATER_LIMITS.bytes))).status, 'invalid-snapshot');
  assert.equal(parse(snapshot('<span>'.repeat(40) + 'deep' + '</span>'.repeat(40))).status, 'invalid-snapshot');
  assert.equal(parse(snapshot('<br>'.repeat(100001))).status, 'invalid-snapshot');
});

test('bounded generated hostile text remains text after HTML entity decoding', () => {
  let seed = 71823;
  const tokens = ['<script>', '</script>', '<img src=x onerror=alert(1)>', '&lt;', '&#x3c;', '"', "'", '&', '😀', '折', ' ', '\n'];
  function comment(tree) {
    if (typeof tree === 'string') return null;
    if (tree.attrs.class === 'postMessage') return tree;
    return tree.children.map(comment).find(Boolean);
  }
  for (let run = 0; run < 256; run++) {
    let text = '';
    for (let part = 0; part < 32; part++) {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
      text += tokens[seed % tokens.length];
    }
    const escaped = text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&#39;');
    const result = parse(snapshot(escaped)); assert.equal(result.status, 'ok');
    const node = comment(result.snapshot.posts[1].tree);
    assert.ok(node.children.every(child => typeof child === 'string'));
    assert.equal(node.children.join(''), text);
  }
});

function workerFactory(record) {
  return () => {
    const worker = { terminate() { record.terminated++; }, postMessage(job) {
      queueMicrotask(() => worker.onmessage?.({ data: parseUpdaterSnapshot(job.raw, job.context) }));
    } }; record.created++; return worker;
  };
}
async function serverFor(t, handler) {
  const server = createServer(handler); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  t.after(async () => { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); });
  return `http://127.0.0.1:${server.address().port}`;
}
test('actual HTTP streams omit credentials, enforce redirects/MIME/byte bounds and terminate parsers', async t => {
  let mode = 'ok', redirected = 0;
  const origin = await serverFor(t, (req, res) => {
    assert.equal(req.headers.cookie, undefined); assert.equal(req.headers.authorization, undefined);
    if (req.url === '/redirect-target') { redirected++; res.end('healthy'); return; }
    if (mode === 'redirect') { res.writeHead(302, { location: '/redirect-target' }); res.end(); return; }
    if (mode === 'missing') { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { 'content-type': mode === 'mime' ? 'text/html' : 'application/json' });
    res.end(mode === 'bad' ? '{' : mode === 'encoding' ? Buffer.from([0xff]) : mode === 'large' ? 'x'.repeat(8192) : JSON.stringify(snapshot()));
  });
  // Healthy allowed-context control for the redirect destination.
  assert.equal(await (await fetch(`${origin}/redirect-target`)).text(), 'healthy');
  const record = { created: 0, terminated: 0 };
  let now = 0;
  const transport = new NativeUpdaterTransport({ ...context, origin, now: () => now, limits: { bytes: 4096 }, createWorker: workerFactory(record),
    fetcher: (url, options) => { assert.equal(options.credentials, 'omit'); assert.equal(options.redirect, 'error'); assert.equal(options.mode, 'same-origin'); return fetch(url, options); } });
  for (const [value, expected] of [['ok', 'ok'], ['redirect', 'network-error'], ['missing', 'http-error'], ['mime', 'invalid-response'],
    ['bad', 'invalid-snapshot'], ['encoding', 'invalid-encoding'], ['large', 'response-limit']]) {
    mode = value; now += 1000;
    assert.equal((await transport.refresh()).status, expected, value);
  }
  assert.equal(redirected, 1); assert.equal(record.created, 2); assert.equal(record.terminated, 2);
});

test('timeout and cancellation settle even when fetch or the reader ignores abort', async () => {
  for (const fetcher of [() => new Promise(() => {}), async url => ({ url, status: 200, headers: new Headers({ 'content-type': 'application/json' }),
    body: new ReadableStream({ start() {} }) })]) {
    const transport = new NativeUpdaterTransport({ ...context, fetcher, limits: { requestMs: 20 } });
    const pending = transport.refresh(); assert.equal((await transport.refresh()).status, 'busy');
    assert.equal((await pending).status, 'timeout'); assert.equal(transport.active, null);
    assert.equal((await transport.refresh()).status, 'cooldown');
    const cancelled = new NativeUpdaterTransport({ ...context, fetcher });
    const work = cancelled.refresh(); cancelled.cancel(); assert.equal((await work).status, 'cancelled');
  }
});

test('hung or hostile worker results are terminated and cannot become DOM instructions', async () => {
  for (const mode of ['hung', 'hostile', 'cancel']) {
    let terminated = 0, transport;
    const createWorker = () => ({ terminate() { terminated++; }, postMessage() {
      if (mode === 'hostile') this.onmessage({ data: { status: 'ok', snapshot: { ...snapshot(), posts: [{ no: context.thread, tree: { tag: 'script', attrs: {}, children: [] } }] } } });
      if (mode === 'cancel') transport.cancel();
    } });
    transport = new NativeUpdaterTransport({ ...context, createWorker, limits: { parseMs: 10 },
      fetcher: async url => ({ url, status: 200, headers: new Headers({ 'content-type': 'application/json' }), body: new Response(JSON.stringify(snapshot())).body }) });
    assert.equal((await transport.refresh()).status, { hung: 'parse-timeout', hostile: 'invalid-snapshot', cancel: 'cancelled' }[mode]);
    assert.equal(terminated, 1); assert.equal(transport.active, null);
  }
});

test('empty chunks cannot keep an updater read alive beyond its finite stream-work budget', async () => {
  let reads = 0, cancels = 0, healthy = false, now = 0;
  const record = { created: 0, terminated: 0 };
  const transport = new NativeUpdaterTransport({ ...context, now: () => now, createWorker: workerFactory(record),
    fetcher: async url => ({ url, status: 200, headers: new Headers({ 'content-type': 'application/json' }),
      body: healthy ? new Response(JSON.stringify(snapshot())).body : { getReader() { return {
        async read() { reads++; return { done: false, value: new Uint8Array() }; },
        async cancel() { cancels++; },
      }; } },
    }),
  });
  assert.equal((await transport.refresh()).status, 'response-limit');
  assert.equal(reads, 65537); assert.equal(cancels, 1);
  assert.equal(record.created, 0); assert.equal(transport.active, null);
  healthy = true; now = 1000;
  assert.equal((await transport.refresh()).status, 'ok');
  assert.equal(record.created, 1); assert.equal(record.terminated, 1);
});

function rangedSnapshot(count, size, tail = false) {
  const ids = Array.from({ length: count + 1 }, (_, i) => String(BigInt(context.thread) + BigInt(i)));
  const s = snapshot(); s.replies = count; s.tail_size = size;
  s.tail_id = tail ? ids[count - size] : null;
  s.posts = (tail ? [ids[0], ...ids.slice(-size)] : ids).map(no => ({ no, file_deleted: false, html: html(no) }));
  return s;
}
const tokenFor = value => `"${createHash('sha256').update(JSON.stringify(value)).digest('hex')}"`;
const modified = 'Mon, 14 Sep 2026 00:00:00 GMT';
function sendSnapshot(req, res, value) {
  const etag = tokenFor(value);
  const headers = { 'content-type': 'application/json', etag, 'last-modified': modified };
  if (req.headers['if-none-match'] === etag) { res.writeHead(304, headers); res.end(); }
  else { res.writeHead(200, headers); res.end(JSON.stringify(value)); }
}

test('tail metadata retains full counts and exact omitted boundaries while rejecting partial or inconsistent representations', () => {
  const s = rangedSnapshot(4, 2, true);
  assert.equal(parse(s).status, 'ok'); assert.equal(s.tail_id, '9007199254740994');
  for (const edit of [s => s.tail_id = s.posts[1].no, s => s.tail_id = context.thread,
    s => s.tail_size = 1, s => s.replies = 3, s => s.tail_id = 0, s => s.tail_size = 1.5,
    s => s.tail_size = 1001, s => s.replies = 1001, s => s.tail_id = '9223372036854775808',
    s => s.tail_id = null, s => delete s.tail_size]) {
    const candidate = structuredClone(s); edit(candidate); assert.equal(parse(candidate).status, 'invalid-snapshot');
  }
  assert.equal(updaterUrl(context, true), 'https://board.example/_watch/demo/thread/9007199254740992/posts-tail');
});

test('tail selection follows reply-window age and uses full responses for disabled, stale or invalid timing', () => {
  assert.equal(useUpdaterTail(2, [1000, 2000, 3000], 10000, 17000), true);
  assert.equal(useUpdaterTail(2, [1000, 2000, 3000], 10000, 18000), false);
  assert.equal(useUpdaterTail(2, [1000], 10000, 100000), true);
  for (const values of [[0, [1000], 10000, 11000], [2, [NaN, 2000], 10000, 11000],
    [2, [1000], 10000, 9999], [1001, [], 10000, 11000]]) assert.equal(useUpdaterTail(...values), false);
  // Avoid signed 32-bit timestamp wrapping after 2038.
  assert.equal(useUpdaterTail(1, [3000000000000], 3000000010000, 3000000011000), true);
});

test('actual full and tail HTTP validators remain separate and 304 avoids parsing or replaying a snapshot', async t => {
  let count = 4, clock = 0; const requests = [], record = { created: 0, terminated: 0 };
  const origin = await serverFor(t, (req, res) => {
    assert.equal(req.headers.cookie, undefined); assert.equal(req.headers.authorization, undefined);
    requests.push({ path: req.url, tag: req.headers['if-none-match'], date: req.headers['if-modified-since'] });
    sendSnapshot(req, res, rangedSnapshot(count, 2, req.url.endsWith('posts-tail')));
  });
  const transport = new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), now: () => clock });
  const known = new Set(rangedSnapshot(4, 2).posts.map(p => p.no));
  assert.equal((await transport.refresh()).status, 'ok'); clock += 1001;
  assert.equal((await transport.refresh()).status, 'not-modified'); clock += 1001;
  assert.equal((await transport.refresh({ tail: true, known })).status, 'ok'); clock += 1001;
  assert.equal((await transport.refresh({ tail: true, known })).status, 'not-modified'); clock += 1001;
  assert.equal(record.created, 2); assert.equal(record.terminated, 2);
  assert.equal(requests[0].date, '0'); assert.equal(requests[0].tag, undefined);
  assert.equal(requests[1].tag, tokenFor(rangedSnapshot(4, 2)));
  assert.equal(requests[2].date, '0'); assert.equal(requests[2].tag, undefined);
  assert.equal(requests[3].tag, tokenFor(rangedSnapshot(4, 2, true))); assert.equal(requests[3].date, modified);
  count = 5;
  assert.equal((await transport.refresh({ tail: true, known })).snapshot.replies, 5);
  transport.invalidate(); clock += 1001;
  assert.equal((await transport.refresh()).status, 'ok'); assert.equal(requests.at(-1).tag, undefined);
});

test('an actual missing tail boundary retries one full response inside the same refresh slot', async t => {
  const requests = [], record = { created: 0, terminated: 0 };
  const origin = await serverFor(t, (req, res) => { requests.push(req.url); sendSnapshot(req, res, rangedSnapshot(6, 2, req.url.endsWith('posts-tail'))); });
  const transport = new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), now: () => 0 });
  const known = new Set(rangedSnapshot(2, 0).posts.map(p => p.no));
  const result = await transport.refresh({ tail: true, known });
  assert.equal(result.status, 'ok'); assert.equal(result.snapshot.tail_id, null); assert.equal(result.snapshot.posts.length, 7);
  assert.deepEqual(requests.map(url => url.split('/').at(-1)), ['posts-tail', 'posts']);
  assert.equal(record.created, 2); assert.equal(record.terminated, 2);
  assert.equal((await transport.refresh()).status, 'cooldown');
});

test('tail fallback shares an aggregate byte ceiling with a healthy complete-response control', async t => {
  const full = rangedSnapshot(6, 2), tail = rangedSnapshot(6, 2, true), record = { created: 0, terminated: 0 };
  const bytes = Buffer.byteLength(JSON.stringify(full)) + Buffer.byteLength(JSON.stringify(tail)) - 1;
  const origin = await serverFor(t, (req, res) => sendSnapshot(req, res, req.url.endsWith('posts-tail') ? tail : full));
  const make = () => new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), limits: { bytes } });
  assert.equal((await make().refresh()).status, 'ok');
  assert.equal((await make().refresh({ tail: true, known: new Set([context.thread]) })).status, 'response-limit');
});

test('tail 404 retries full, full 404 is terminal, and other errors or unsolicited 304 do not become success', async t => {
  let mode = 'missing-tail', clock = 0; const requests = [], record = { created: 0, terminated: 0 };
  const origin = await serverFor(t, (req, res) => {
    requests.push(req.url);
    if (mode === 'unsolicited') { res.writeHead(304); res.end(); }
    else if (mode === 'missing-all' || (mode === 'missing-tail' && req.url.endsWith('posts-tail'))) { res.writeHead(404); res.end(); }
    else if (mode === 'failure') { res.writeHead(503); res.end(); }
    else sendSnapshot(req, res, rangedSnapshot(4, 2));
  });
  const transport = new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), now: () => clock });
  const options = { tail: true, known: new Set([context.thread]) };
  assert.equal((await transport.refresh(options)).status, 'ok'); assert.equal(requests.length, 2); clock += 1001;
  mode = 'missing-all'; assert.deepEqual(await transport.refresh(options), { status: 'http-error', httpStatus: 404 }); assert.equal(requests.length, 4); clock += 1001;
  mode = 'failure'; assert.deepEqual(await transport.refresh(options), { status: 'http-error', httpStatus: 503 }); assert.equal(requests.length, 5); clock += 1001;
  transport.invalidate(); mode = 'unsolicited'; assert.equal((await transport.refresh()).status, 'invalid-response');
});

test('a hung full fallback settles at the original deadline and a fresh healthy request still works', async t => {
  let hang = false, clock = 0; const requests = [], record = { created: 0, terminated: 0 };
  const origin = await serverFor(t, (req, res) => {
    requests.push(req.url);
    if (hang && !req.url.endsWith('posts-tail')) return;
    sendSnapshot(req, res, rangedSnapshot(6, 2, req.url.endsWith('posts-tail')));
  });
  const transport = new NativeUpdaterTransport({ ...context, origin, createWorker: workerFactory(record), now: () => clock, limits: { requestMs: 150 } });
  assert.equal((await transport.refresh()).status, 'ok'); clock += 1001; hang = true;
  assert.equal((await transport.refresh({ tail: true, known: new Set([context.thread]) })).status, 'timeout');
  assert.equal(requests.length, 3); clock += 1001; hang = false;
  assert.equal((await transport.refresh()).status, 'not-modified');
  assert.equal(requests.length, 4);
});
