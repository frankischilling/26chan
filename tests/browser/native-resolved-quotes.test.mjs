import { withFailurePreservingCleanup } from './helpers/preserve-cleanup-failure.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { parseUpdaterSnapshot, parseQuotePreviewSnapshot, parseBoardPageSnapshot,
  postLinkUrl, parsePostRecipe, validatePostTree, UPDATER_LIMITS, PREVIEW_LIMITS } from '../../apps/public/client/native-updater-snapshot.js';
import { quoteTarget, localQuoteTree, prepareQuotePost } from '../../apps/public/client/native-quote-preview.js';
import { parseSearchPayload } from '../../apps/public/client/global-search.js';

const context = { origin: 'https://board.example', board: 'g', thread: '9007199254740992' };
const no = context.thread, target = '9007199254740993';
function post(comment) {
  return { no, file_deleted: false, html: `<article class="postContainer opContainer" id="pc${no}"><div class="post op" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span></div><blockquote class="postMessage" id="m${no}">${comment}</blockquote></div></article>` };
}
const updater = value => ({ version: 2, board: context.board, thread: no, closed: false, archived: false,
  sticky: false, replies: 0, images: 0, posts: [value], tail_size: 0, tail_id: null });
const preview = value => ({ version: 1, board: context.board, thread: no, post: value });
const page = value => ({ version: 2, board: context.board, page: 0, next_page: null, replies_shown: 3, threads: [{ thread: no,
  closed: false, sticky: false, archived: false, replies: 0, images: 0, omitted: 0, posts: [value] }] });
const search = value => ({ threads: [{ board: context.board, thread: no, posts: [{ no, html: value.html }] }], offset: 0, nhits: 1 });
const message = tree => tree.children[0].children[1];
const parse = comment => parseUpdaterSnapshot(JSON.stringify(updater(post(comment))), context);
const live = href => `<a class="quotelink" href="${href}">&gt;&gt;${target}</a>`;
const dead = '<span class="deadlink">&gt;&gt;&gt;/co/123 &lt;script&gt; &amp; text</span>';

test('source-resolved fragments, thread anchors and dead labels survive all actual safe-tree parsers', () => {
  const comment = live(`#p${target}`) + live(`/co/thread/100#p200`) + dead;
  const value = post(comment);
  const trees = [
    parse(comment).snapshot.posts[0].tree,
    parseQuotePreviewSnapshot(JSON.stringify(preview(value)), { ...context, post: no }).snapshot.post.tree,
    parseBoardPageSnapshot(JSON.stringify(page(value)), { ...context, page: 0 }).snapshot.threads[0].posts[0].tree,
    parseSearchPayload(JSON.stringify(search(value)), context).threads[0].posts[0].tree,
  ];
  for (const tree of trees) {
    assert.doesNotThrow(() => validatePostTree(tree, context, no));
    const children = message(tree).children;
    assert.equal(children[0].attrs.href, `#p${target}`);
    assert.equal(children[1].attrs.href, '/co/thread/100#p200');
    assert.deepEqual(children[2], { tag: 'span', attrs: { class: 'deadlink' }, children: ['>>>/co/123 <script> & text'] });
    assert.deepEqual(prepareQuotePost(tree, context, no).quotes, [`/g/thread/${no}#p${target}`, '/co/thread/100#p200']);
  }
});

test('bare fragments require exact positive i64 identity and an identified containing thread', () => {
  for (const id of [no, target, '9223372036854775807']) {
    assert.equal(postLinkUrl(`#p${id}`, context), true);
    assert.deepEqual(quoteTarget(`#p${id}`, context), { board: 'g', thread: no, post: id });
  }
  for (const href of ['#p0', '#p01', '#p+1', '#p-1', '#p1.0', '#p1e2', '#p%31', '#P1', '#p',
    `#p${target}\n`, `#p${target}\r`,
    `#p${target}\t`, `#p${target}#p1`, `#p${target}?x=1`, `#p${target}/`, '#p1']) {
    assert.equal(postLinkUrl(href, context), false, href);
    assert.equal(quoteTarget(href, context), null, href);
    assert.equal(parse(live(href)).status, 'invalid-snapshot', href);
  }
  for (const patch of [{ thread: null }, { thread: undefined }, { thread: '01' }, { thread: '1\n' }, { thread: 1 },
    { thread: '9223372036854775808' }, { thread: '9223372036854775807' },
    { board: 'g\n' }, { board: '../g' }, { board: '' }]) {
    assert.equal(postLinkUrl(`#p${target}`, { ...context, ...patch }), false);
    assert.equal(quoteTarget(`#p${target}`, { ...context, ...patch }), null);
  }
  for (const href of ['//evil.example/g/thread/1#p2', 'https://evil.example/g/thread/1#p2',
    '/g/thread/2#p1', '/g/thread/00#p2', '/g/post/9223372036854775808',
    '/g/../co/thread/1#p2', '/g/thread/%31#p2', '/g/thread/1#p2?x=1']) {
    assert.equal(quoteTarget(href, context), null, href);
  }
  for (const spelling of ['9223372036854775808', '99999999999999999999', '+9223372036854775807', '1e19']) {
    assert.equal(postLinkUrl(`#p${spelling}`, context), true);
    assert.equal(parse(live(`#p${spelling}`)).status, 'ok');
  }
  assert.equal(parse(`<a href="#p${target}">ordinary link</a>`).status, 'invalid-snapshot');
  const tree = parse(live(`#p${target}`)).snapshot.posts[0].tree;
  tree.children[0].children[0].children.push(message(tree).children.pop());
  assert.throws(() => validatePostTree(tree, context, no), 'fragments do not authorize header links');
});

test('dead spans cannot acquire active descendants, attributes or action-bearing classes', () => {
  for (const invalid of [
    '<a class="deadlink" href="/g/post/123">&gt;&gt;123</a>',
    `<a class="quotelink" href="/g/post/123">${dead}</a>`,
    `<a href="/g/post/123"><s>${dead}</s></a>`,
    '<span class="deadlink quotelink">&gt;&gt;123</span>',
    '<span class="quote deadlink">&gt;&gt;123</span>',
    '<span class="deadlink " >&gt;&gt;123</span>',
    ...['onclick="bad()"', 'style="background:url(https://evil.example/)"', 'href="/g/post/123"',
      'src="https://evil.example/"', 'data-action="quote"', 'tabindex="0"', 'aria-label="Spoiler; focus to reveal"',
      'title="dead"', `id="f${no}"`].map(attr => `<span class="deadlink" ${attr}>&gt;&gt;123</span>`),
    ...[live(`/g/thread/${no}#p${target}`), '<b>text</b>', '<script>bad()</script>',
      '<img src="https://evil.example/" alt="">', '<iframe src="https://evil.example/"></iframe>',
      '<sp4n cl4ss="sjis">text</sp4n>'].map(child => `<span class="deadlink">${child}</span>`),
  ]) {
    const value = post(invalid);
    assert.equal(parse(invalid).status, 'invalid-snapshot', invalid);
    assert.equal(parseQuotePreviewSnapshot(JSON.stringify(preview(value)), { ...context, post: no }).status, 'invalid-preview', invalid);
    assert.equal(parseBoardPageSnapshot(JSON.stringify(page(value)), { ...context, page: 0 }).status, 'invalid-snapshot', invalid);
    assert.throws(() => parseSearchPayload(JSON.stringify(search(value)), context), undefined, invalid);
  }
  const tree = parse(dead).snapshot.posts[0].tree;
  tree.children[0].children[0].children.push(message(tree).children.pop());
  assert.throws(() => validatePostTree(tree, context, no), 'deadlink is comment-only');
});

// Small DOM adapter exercises the actual local recipe reader without Chromium.
function dom(node) {
  if (typeof node === 'string') return { nodeType: 3, data: node };
  const attrs = { ...node.attrs };
  return { nodeType: 1, namespaceURI: 'http://www.w3.org/1999/xhtml', localName: node.tag,
    className: attrs.class ?? '', id: attrs.id ?? '', classList: (attrs.class ?? '').split(' '),
    attributes: Object.entries(attrs).map(([name, value]) => ({ name, value })),
    childNodes: node.children.map(dom), matches: () => false, closest: () => null,
    getAttribute: key => attrs[key] ?? null, hasAttribute: key => Object.hasOwn(attrs, key) };
}

test('local recipe extraction preserves dead labels and binds fragments to canonical destinations', () => {
  const tree = parse(live(`#p${target}`) + dead).snapshot.posts[0].tree;
  const local = localQuoteTree(dom(tree), context, no);
  assert.equal(message(local).children[0].attrs.href, `/g/thread/${no}#p${target}`);
  assert.deepEqual(message(local).children[1], message(tree).children[1]);
  assert.deepEqual(prepareQuotePost(local, context, no).quotes, [`/g/thread/${no}#p${target}`]);
  const onlyDead = localQuoteTree(dom(parse(dead).snapshot.posts[0].tree), context, no);
  assert.deepEqual(prepareQuotePost(onlyDead, context, no).quotes, []);
});

test('new quote recipes preserve node, depth and character budgets on parser and revalidation paths', () => {
  for (const limits of [UPDATER_LIMITS, PREVIEW_LIMITS]) {
    const html = post(dead).html;
    assert.throws(() => parsePostRecipe(html, context, no, { nodes: limits.nodes }, limits));
    assert.throws(() => parsePostRecipe(html, context, no, { nodes: 0 }, { ...limits, depth: 2 }));
    assert.throws(() => parsePostRecipe(html, context, no, { nodes: 0, chars: limits.bytes }, limits));
    const tree = parse(dead).snapshot.posts[0].tree;
    assert.throws(() => validatePostTree(tree, context, no, { nodes: limits.nodes }, limits));
    assert.throws(() => validatePostTree(tree, context, no, { nodes: 0 }, { ...limits, depth: 2 }));
    assert.throws(() => validatePostTree(tree, context, no, { nodes: 0, chars: limits.bytes }, limits));
  }
});


test('no-JavaScript quote lifecycle keeps hidden credentials on the real anonymous-session path', async () => {
  const source = await readFile(new URL('./behavior.spec.js', import.meta.url), 'utf8');
  const start = source.indexOf("test('cross-board quotes navigate persisted replies and respect deletion without JavaScript'");
  assert.notEqual(start, -1);
  const end = source.indexOf('\n});', start);
  assert.ok(end > start);
  const fixture = source.slice(start, end);
  const fields = await readFile(new URL('../../apps/public/templates/post_fields.html', import.meta.url), 'utf8');
  const post = await readFile(new URL('../../apps/public/templates/post_content.html', import.meta.url), 'utf8');
  assert.match(fields, /id="postPassword" name="pwd" type="hidden"/);
  assert.match(post, /id="delete\{\{ item\.post\.id \}\}" name="password" type="hidden"/);
  assert.match(fixture, /javaScriptEnabled: false/);
  assert.match(fixture, /cookie\.name === 'board-anon'/);
  assert.match(fixture, /httpOnly: true/);
  assert.match(fixture, /await post\('\/tg\/'/);
  assert.match(fixture, /await post\('\/fixture\/'/);
  assert.match(fixture, /deletionFixture\('age', 'tg', reply, marker\)/);
  assert.match(fixture, /name: 'Delete post', exact: true/);
  assert.doesNotMatch(fixture, /(?:postPassword|delete\$\{reply\})[^\n]*\.(?:fill|evaluate|pressSequentially)\(/,
    'Hidden posting and deletion fields must not be edited by the browser fixture');
  assert.doesNotMatch(fixture, /postTarget|pwd: password|form: \{ no, password \}/,
    'Quote lifecycle setup and cleanup must retain automatic session ownership');
});


test('quote lifecycle cleanup preserves primary failures and never turns teardown failures into success', async () => {
  const primary = new Error('action failed'), secondary = new Error('cleanup failed');
  const recorded = [];
  await assert.rejects(withFailurePreservingCleanup(async () => { throw primary; },
    async () => { throw secondary; }, error => recorded.push(error)), error => error === primary);
  assert.deepEqual(recorded, [secondary]);
  await assert.rejects(withFailurePreservingCleanup(async () => 'ok',
    async () => { throw secondary; }, error => recorded.push(error)), error => error === secondary);
  assert.deepEqual(recorded, [secondary], 'A sole cleanup failure is thrown, not downgraded');
  await assert.rejects(withFailurePreservingCleanup(async () => { throw primary; },
    async () => {}, error => recorded.push(error)), error => error === primary);
  assert.equal(await withFailurePreservingCleanup(async () => 'ok', async () => {}), 'ok');
  await assert.rejects(withFailurePreservingCleanup(async () => { throw primary; },
    async () => { throw secondary; }, () => { throw new Error('diagnostic failure'); }), error => error === primary);
});

test('opt-in Pass badges survive remote and local quote recipes without broadening markup', async () => {
  const released = await import('../../apps/public/static/native-filter.v1.js');
  const searchRelease = await import('../../apps/public/static/global-search.v1.js');
  for (const year of [1, 2020, 9999]) {
    const value = post('Owned member');
    value.html = value.html.replace('<span class="name">Anonymous</span>', `<span class="name">Anonymous</span> <span title="Pass user since ${year}" class="n-pu"></span>`);
    const parsed = parseUpdaterSnapshot(JSON.stringify(updater(value)), context);
    assert.equal(parsed.status, 'ok');
    const tree = parsed.snapshot.posts[0].tree;
    assert.doesNotThrow(() => released.validatePostTree(tree, context, no));
    for (const reader of [localQuoteTree, released.localQuoteTree]) {
      const local = reader(dom(tree), context, no);
      const badge = local.children[0].children[0].children.find(node => node.attrs?.class === 'n-pu');
      assert.deepEqual(badge, { tag: 'span', attrs: { class: 'n-pu', title: `Pass user since ${year}` }, children: [] });
    }
    assert.equal(parseQuotePreviewSnapshot(JSON.stringify(preview(value)), { ...context, post: no }).status, 'ok');
    assert.equal(parseBoardPageSnapshot(JSON.stringify(page(value)), { ...context, page: 0 }).status, 'ok');
    assert.doesNotThrow(() => parseSearchPayload(JSON.stringify(search(value)), context));
    assert.doesNotThrow(() => searchRelease.parseSearchPayload(JSON.stringify(search(value)), context));
  }
  for (const badge of [
    '<span title="Pass user since 0" class="n-pu"></span>',
    '<span title="Pass user since 10000" class="n-pu"></span>',
    '<span title="Pass user since 02020" class="n-pu"></span>',
    '<span title="Pass user since 2020" class="n-pu name"></span>',
    '<span title="Pass user since 2020" class="n-pu">text</span>',
    '<span title="Pass user since 2020" class="n-pu" onclick="owned()"></span>',
    '<span class="n-pu"></span>',
    '<a title="Pass user since 2020" class="n-pu" href="/g/post/123"></a>',
  ]) {
    const value = post('Owned');
    value.html = value.html.replace('<span class="name">Anonymous</span>', badge);
    assert.equal(parseUpdaterSnapshot(JSON.stringify(updater(value)), context).status, 'invalid-snapshot', badge);
  }
  assert.equal(parse('<span title="Pass user since 2020" class="n-pu"></span>').status, 'invalid-snapshot', 'comments cannot claim membership');
});

test('stored source stock badges remain finite inert identity markup in every recipe', async () => {
  const fixture = JSON.parse(await readFile(new URL('../../fixtures/event-reference.json', import.meta.url), 'utf8'));
  const rows = fixture.cases.stock.filter(row => row.stock);
  assert.equal(rows.length, 18);
  const released = await import('../../apps/public/static/native-filter.v1.js');
  const searchRelease = await import('../../apps/public/static/global-search.v1.js');
  const stockPost = row => {
    const value = post('Owned stock identity');
    value.html = value.html.replace('postContainer opContainer', `postContainer opContainer${row.class}`)
      .replace('<span class="name">Anonymous</span>', `<span class="name">Anonymous</span>${row.badge}${row.badge}`);
    return value;
  };
  for (const row of rows) {
    const value = stockPost(row);
    const parsed = parseUpdaterSnapshot(JSON.stringify(updater(value)), context);
    assert.equal(parsed.status, 'ok', row.stock);
    const tree = parsed.snapshot.posts[0].tree;
    assert.doesNotThrow(() => released.validatePostTree(tree, context, no));
    for (const reader of [localQuoteTree, released.localQuoteTree]) {
      const local = reader(dom(tree), context, no);
      assert.equal(local.attrs.class, `postContainer opContainer${row.class}`);
      const badges = local.children[0].children[0].children.filter(node => node.attrs?.['data-tip']);
      assert.equal(badges.length, 2);
      assert.ok(badges.every(node => node.attrs.class === `n-xa24 n-xa24-${row.stock}` && node.attrs['data-tip'] === row.stock && node.children.length === 0));
    }
    assert.equal(parseQuotePreviewSnapshot(JSON.stringify(preview(value)), { ...context, post: no }).status, 'ok');
    assert.equal(parseBoardPageSnapshot(JSON.stringify(page(value)), { ...context, page: 0 }).status, 'ok');
    assert.doesNotThrow(() => parseSearchPayload(JSON.stringify(search(value)), context));
    assert.doesNotThrow(() => searchRelease.parseSearchPayload(JSON.stringify(search(value)), context));
  }
  const good = stockPost(rows[0]);
  for (const change of [
    html => html.replaceAll('PEPE', 'UNKNOWN'),
    html => html.replaceAll('PEPE', 'pepe'),
    html => html.replace('p-xa24-PEPE', 'p-xa24-WOJK'),
    html => html.replace('data-tip="PEPE"', 'data-tip="WOJK"'),
    html => html.replace('data-tip="PEPE"', 'data-tip="PEPE" onclick="owned()"'),
    html => html.replace(rows[0].badge, ''),
    html => html.replace(rows[0].badge, rows[0].badge.repeat(2)),
    html => html.replace(' p-xa24-PEPE', ''),
    html => html.replace('Owned stock identity', rows[0].badge),
    html => html.replace('Owned stock identity', '<span class="n-pu" title="Pass user since 2020"></span>'),
    html => html.replace('<span class="name">Anonymous</span>', '<span class="name">Anonymous</span><span class="n-pu" title="Pass user since 2020"></span>'),
  ]) {
    const value = { ...good, html: change(good.html) };
    assert.equal(parseUpdaterSnapshot(JSON.stringify(updater(value)), context).status, 'invalid-snapshot');
  }
  assert.equal(parse(rows[0].badge).status, 'invalid-snapshot');
});

test('optional perks retain exactly the pinned Unicode recipes without active content', async () => {
  const fixture = JSON.parse(await readFile(new URL('../../fixtures/event-perk-reference.json', import.meta.url), 'utf8'));
  const { eventPerkRecipe } = await import('../../apps/public/client/native-event-perks.js');
  const released = await import('../../apps/public/static/native-filter.v1.js');
  const searchRelease = await import('../../apps/public/static/global-search.v1.js');
  const allowed = new Set(fixture.cases.map(row => row.glyph));
  for (let code = 0; code <= 0x10ffff; ++code) {
    const glyph = String.fromCodePoint(code);
    assert.equal(eventPerkRecipe('div', { class: 'like-perk-cnt' }, [glyph]), allowed.has(glyph));
  }
  for (const row of fixture.cases) {
    const value = post(row.html);
    const parsed = parseUpdaterSnapshot(JSON.stringify(updater(value)), context);
    assert.equal(parsed.status, 'ok', row.name);
    const tree = parsed.snapshot.posts[0].tree;
    assert.doesNotThrow(() => released.validatePostTree(tree, context, no));
    assert.deepEqual(message(tree).children[0].children, [row.glyph]);
    for (const reader of [localQuoteTree, released.localQuoteTree]) {
      const local = reader(dom(tree), context, no);
      assert.equal(message(local).children[0].attrs.class, 'like-perk-cnt');
    }
    assert.equal(parseQuotePreviewSnapshot(JSON.stringify(preview(value)), { ...context, post: no }).status, 'ok');
    assert.equal(parseBoardPageSnapshot(JSON.stringify(page(value)), { ...context, page: 0 }).status, 'ok');
    assert.doesNotThrow(() => searchRelease.parseSearchPayload(JSON.stringify(search(value)), context));
  }
  for (const html of [
    '<div class="like-perk-cnt">owned</div>',
    '<div class="like-perk-cnt"><b>😃</b></div>',
    '<div class="like-perk-cnt" onclick="owned()">😃</div>',
    '<div class="like-perk-cnt"><img src="/owned"></div>',
    '<span class="like-perk-cnt">😃</span>',
    '<a href="/g/thread/123"><div class="like-perk-cnt">😃</div></a>',
  ]) assert.equal(parse(html).status, 'invalid-snapshot', html);
  const value = post('Owned');
  value.html = value.html.replace('<span class="name">Anonymous</span>', '<div class="like-perk-cnt">😃</div>');
  assert.equal(parseUpdaterSnapshot(JSON.stringify(updater(value)), context).status, 'invalid-snapshot');
});
