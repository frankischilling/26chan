import { defaultTreeAdapter, parseFragment } from 'parse5';
import { postId } from '../static/thread-watcher-core.v1.js';

export const UPDATER_LIMITS = Object.freeze({ bytes: 4194304, posts: 1001, nodes: 100000,
  depth: 32, requestMs: 10000, parseMs: 2000, intervalMs: 1000 });
const classes = new Set(['postContainer', 'opContainer', 'replyContainer', 'sideArrows', 'post',
  'op', 'reply', 'postInfo', 'subject', 'name', 'postNum', 'file', 'fileThumb', 'fileDeleted',
  'postMessage', 'quote', 'quotelink', 'spoiler', 'sjis', 'prettyprint', 'postActions']);
const attributes = {
  article: ['class', 'id'], div: ['class', 'id', 'aria-hidden'], span: ['class', 'tabindex', 'aria-label'],
  time: ['datetime'], a: ['class', 'href', 'target', 'rel'], blockquote: ['class', 'id'],
  br: [], s: [], pre: ['class'], p: ['class'], details: ['class'], summary: [], form: ['method', 'action'],
  input: ['type', 'name', 'value', 'id', 'minlength', 'maxlength', 'autocomplete', 'required'],
  label: ['for'], button: [], img: ['src', 'alt', 'width', 'height', 'loading'],
};
function require(value) { if (!value) throw new TypeError('invalid-snapshot'); }
function exactKeys(value, keys) {
  require(value && typeof value === 'object' && !Array.isArray(value));
  require(Object.keys(value).sort().join(',') === [...keys].sort().join(','));
}
export function updaterContext({ origin, board, thread, mediaOrigin = '' }) {
  const url = new URL(origin);
  require(['http:', 'https:'].includes(url.protocol) && url.href === `${url.origin}/` && !url.username && !url.password);
  require(typeof board === 'string' && /^[a-z0-9]{1,10}$/.test(board) && typeof thread === 'string' && postId(thread) === thread);
  if (mediaOrigin) {
    const media = new URL(mediaOrigin);
    require(['http:', 'https:'].includes(media.protocol) && media.href === `${media.origin}/` && !media.username && !media.password);
    mediaOrigin = media.origin;
  }
  return { origin: url.origin, board, thread, mediaOrigin };
}
export function updaterUrl(context, tail = false) {
  const { origin, board, thread } = updaterContext(context);
  require(typeof tail === 'boolean');
  return `${origin}/_watch/${board}/thread/${thread}/posts${tail ? '-tail' : ''}`;
}

export function validateSnapshotMetadata(snapshot, context) {
  exactKeys(snapshot, ['version', 'board', 'thread', 'closed', 'archived', 'sticky', 'replies', 'images', 'posts', 'tail_size', 'tail_id']);
  require(snapshot.version === 2 && snapshot.board === context.board && snapshot.thread === context.thread);
  for (const key of ['closed', 'archived', 'sticky']) require(typeof snapshot[key] === 'boolean');
  require(Array.isArray(snapshot.posts) && snapshot.posts.length > 0 && snapshot.posts.length <= UPDATER_LIMITS.posts);
  require(Number.isInteger(snapshot.replies) && snapshot.replies >= 0 && snapshot.replies < UPDATER_LIMITS.posts
    && Number.isInteger(snapshot.images) && snapshot.images >= 0 && snapshot.images <= snapshot.replies);
  require(Number.isInteger(snapshot.tail_size) && snapshot.tail_size >= 0 && snapshot.tail_size < UPDATER_LIMITS.posts
    && (snapshot.tail_size === 0 || snapshot.replies >= snapshot.tail_size * 2));
  if (snapshot.tail_id === null) require(snapshot.replies === snapshot.posts.length - 1);
  else {
    require(postId(snapshot.tail_id) === snapshot.tail_id && BigInt(snapshot.tail_id) > BigInt(context.thread)
      && snapshot.tail_size > 0 && snapshot.posts.length === snapshot.tail_size + 1
      && postId(snapshot.posts[1]?.no) === snapshot.posts[1]?.no
      && BigInt(snapshot.posts[1].no) > BigInt(snapshot.tail_id));
  }
}
function mediaUrl(raw, context) {
  if (!context.mediaOrigin) return false;
  const prefix = `${context.mediaOrigin}/${context.board}/`;
  return raw.startsWith(prefix) && /^[1-9][0-9]{0,18}(?:\.png|s\.jpg)$/.test(raw.slice(prefix.length));
}
function linkUrl(raw, context) {
  if (raw.startsWith('/')) return /^\/[a-z0-9]{1,10}\/(?:post\/[1-9][0-9]{0,18}|thread\/[1-9][0-9]{0,18}(?:#p[1-9][0-9]{0,18})?)$/.test(raw);
  const url = new URL(raw);
  return ['http:', 'https:'].includes(url.protocol) && !url.username && !url.password && !/[\u0000-\u0020\u007f]/.test(raw);
}
// Validate again before DOM construction. Only these inert tags/attributes can
// cross from the worker's data tree into the browser, even with a bad response.
export function validatePostTree(tree, context, no, budget = { nodes: 0 }) {
  budget.chars ??= 0;
  const charge = text => { budget.chars += text.length; require(budget.chars <= UPDATER_LIMITS.bytes); };
  const ids = new Set(), expectedIds = new Set(['pc', 'sa', 'p', 'pi', 'm', 'f', 'delete', 'report'].map(prefix => prefix + no));
  function visit(node, depth, form = null) {
    require(++budget.nodes <= UPDATER_LIMITS.nodes && depth <= UPDATER_LIMITS.depth);
    if (typeof node === 'string') { charge(node); return; }
    exactKeys(node, ['tag', 'attrs', 'children']);
    require(Object.hasOwn(attributes, node.tag) && Array.isArray(node.children));
    require(node.attrs && typeof node.attrs === 'object' && !Array.isArray(node.attrs));
    for (const [key, value] of Object.entries(node.attrs)) {
      // Rust permits 64 KiB of comment UTF-8; URL percent encoding can triple
      // that size. Preserve those valid links within the aggregate wire budget.
      require(attributes[node.tag].includes(key) && typeof value === 'string' && value.length <= (key === 'href' ? 192000 : 4096));
      charge(value);
      if (key === 'class') require(value.split(' ').every(token => classes.has(token)));
      if (key === 'id') { require(expectedIds.has(value) && !ids.has(value)); ids.add(value); }
      if (key === 'href') require(linkUrl(value, context));
      if (key === 'src') require(mediaUrl(value, context));
      if (key === 'action') require([`/${context.board}/delete`, `/${context.board}/report`].includes(value));
      if (key === 'method') require(value === 'post');
      if (key === 'target') require(value === '_blank');
      if (key === 'rel') require(['noopener noreferrer', 'nofollow noreferrer noopener'].includes(value));
      if (key === 'tabindex') require(value === '0');
      if (key === 'aria-hidden') require(value === 'true');
      if (key === 'aria-label') require(value === 'Spoiler; focus to reveal');
      if (key === 'loading') require(value === 'lazy');
      if (key === 'for') require([`delete${no}`, `report${no}`].includes(value));
      if (['width', 'height', 'minlength', 'maxlength'].includes(key)) require(/^[1-9][0-9]{0,3}$/.test(value));
      if (key === 'autocomplete') require(value === 'off');
      if (key === 'required') require(value === '');
    }
    if (node.tag === 'form') {
      require(form === null && node.attrs.method === 'post' && ['delete', 'report'].some(action => node.attrs.action === `/${context.board}/${action}`));
      form = node.attrs.action;
    }
    if (node.tag === 'input') {
      const a = node.attrs;
      require(form !== null);
      require((a.type === 'hidden' && a.name === 'no' && a.value === no && !a.id)
        || (form.endsWith('/delete') && a.type === 'password' && a.name === 'password' && a.id === `delete${no}` && a.value === undefined)
        || (form.endsWith('/delete') && a.type === 'checkbox' && a.name === 'file_only' && a.value === 'true' && !a.id)
        || (form.endsWith('/report') && !a.type && a.name === 'reason' && a.id === `report${no}` && a.value === undefined));
    }
    if (node.tag === 'button') require(form !== null);
    if (node.tag === 'pre') require(node.attrs.class === 'prettyprint');
    if (node.tag === 'img') require(typeof node.attrs.src === 'string' && typeof node.attrs.alt === 'string');
    if (node.tag === 'a') {
      require(typeof node.attrs.href === 'string');
      if (!node.attrs.href.startsWith('/')) require(['noopener noreferrer', 'nofollow noreferrer noopener'].includes(node.attrs.rel));
    }
    for (const child of node.children) visit(child, depth + 1, form);
  }
  visit(tree, 0);
  require(tree.tag === 'article' && tree.attrs.id === `pc${no}`
    && tree.attrs.class === `postContainer ${no === context.thread ? 'opContainer' : 'replyContainer'}`);
  for (const prefix of ['pc', 'p', 'pi', 'm']) require(ids.has(prefix + no));
  return tree;
}

// Runs only in a disposable worker. parse5 creates data, never live DOM, so a
// rejected img, SVG, style or script cannot initiate a request while parsing.
export function parseUpdaterSnapshot(raw, inputContext) {
  try {
    const context = updaterContext(inputContext);
    require(typeof raw === 'string' && raw.length <= UPDATER_LIMITS.bytes && new TextEncoder().encode(raw).length <= UPDATER_LIMITS.bytes);
    const snapshot = JSON.parse(raw);
    validateSnapshotMetadata(snapshot, context);
    let previous = 0n, created = 0;
    const budget = { nodes: 0 };
    const treeAdapter = { ...defaultTreeAdapter, createElement(...args) {
      require(++created <= UPDATER_LIMITS.nodes); return defaultTreeAdapter.createElement(...args);
    } };
    const posts = snapshot.posts.map((post, index) => {
      exactKeys(post, ['no', 'file_deleted', 'html']);
      require(postId(post.no) === post.no && BigInt(post.no) > previous && (index !== 0 || post.no === context.thread));
      previous = BigInt(post.no);
      require(typeof post.file_deleted === 'boolean' && typeof post.html === 'string');
      const fragment = parseFragment(post.html, { treeAdapter, scriptingEnabled: true });
      function recipe(node, depth) {
        require(depth <= UPDATER_LIMITS.depth);
        if (node.nodeName === '#text') return node.value;
        require(node.namespaceURI === 'http://www.w3.org/1999/xhtml' && Object.hasOwn(attributes, node.tagName));
        return { tag: node.tagName, attrs: Object.fromEntries(node.attrs.map(a => { require(!a.namespace && !a.prefix); return [a.name, a.value]; })),
          children: node.childNodes.map(child => recipe(child, depth + 1)) };
      }
      const roots = fragment.childNodes.filter(node => node.nodeName !== '#text' || node.value.trim() !== '');
      require(roots.length === 1);
      const tree = validatePostTree(recipe(roots[0], 0), context, post.no, budget);
      return { no: post.no, file_deleted: post.file_deleted, tree };
    });
    return { status: 'ok', snapshot: { ...snapshot, posts } };
  } catch { return { status: 'invalid-snapshot' }; }
}
