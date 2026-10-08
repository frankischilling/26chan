import { defaultTreeAdapter, parseFragment } from 'parse5';
import { isPostFlagClass, isPostFlagToken } from './native-post-flags.js';
import { isCapcodeToken, postIdentityUrl, validateCapcodeTree } from './native-capcodes.js';
import { validatePostNumbers } from './native-post-numbers.js';
import { postFileAssetUrl, validateFilePresentation } from './native-file-presentation.js';
import { postId } from '../static/thread-watcher-core.v1.js';
import { isWordfilterMarkup, isWordfilterMarkupTag } from './native-wordfilter-markup.js';

export const UPDATER_LIMITS = Object.freeze({ bytes: 4194304, posts: 1001, nodes: 100000,
  depth: 32, requestMs: 10000, parseMs: 2000, intervalMs: 1000 });
export const PREVIEW_LIMITS = Object.freeze({ bytes: 262144, nodes: 16384, depth: 32,
  requestMs: 5000, parseMs: 1000, intervalMs: 300, companions: 4096 });
const classes = new Set(['postContainer', 'opContainer', 'replyContainer', 'sideArrows', 'post',
  'op', 'reply', 'postInfo', 'postInfoM', 'mobile', 'dateTime', 'subject', 'name', 'postertrip', 'posteruid', 'hand', 'postNum', 'file', 'fileText', 'mFileInfo', 'fileThumb', 'imgspoiler', 'fileDeleted', 'fileDeletedRes',
  'postMessage', 'quote', 'quotelink', 'spoiler', 'sjis', 'mu-s', 'mu-i', 'mu-r', 'mu-g', 'mu-b', 'prettyprint', 'postActions',
  'fortune', 'fortune-0', 'fortune-1', 'fortune-2', 'fortune-3', 'fortune-4', 'fortune-5', 'fortune-6',
  'fortune-7', 'fortune-8', 'fortune-9', 'fortune-10', 'fortune-11', 'fortune-12']);
const attributes = {
  article: ['class', 'id', 'data-custom-spoiler'], div: ['class', 'id', 'title', 'aria-hidden', 'data-image-spoiler', 'data-image-filename', 'data-thumbnail-width', 'data-thumbnail-height', 'data-thumbnail-legacy'], span: ['class', 'tabindex', 'aria-label', 'title', 'data-utc'],
  strong: ['class', 'title'], time: ['datetime'], a: ['class', 'href', 'target', 'rel', 'title'], blockquote: ['class', 'id'],
  br: [], wbr: [], b: [], s: [], pre: ['class'], p: ['class'], details: ['class'], summary: [], form: ['method', 'action'],
  input: ['type', 'name', 'value', 'id', 'minlength', 'maxlength', 'autocomplete', 'required'],
  label: ['for'], button: [], img: ['class', 'src', 'srcset', 'alt', 'title', 'width', 'height', 'loading'],
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
export function postMediaUrl(raw, context) {
  if (!context.mediaOrigin) return false;
  const prefix = `${context.mediaOrigin}/${context.board}/`;
  return raw.startsWith(prefix) && /^[1-9][0-9]{0,18}(?:\.png|s\.jpg)$/.test(raw.slice(prefix.length));
}
export function postLinkUrl(raw, context) {
  if (raw.startsWith('/')) {
    if (/^\/rules#[a-z0-9]{1,10}[a-z0-9+/,\-]*$/.test(raw)) return true;
    // OP Reply/View thread uses the server's ordinary semantic context. Match
    // the raw root-relative path, never a URL-normalized or decoded alias, and
    // retain the exact decimal identity across worker/main-thread validation.
    const semantic = /^\/([a-z0-9]{1,10})\/thread\/([1-9][0-9]{0,18})\/([a-z0-9]+(?:-[a-z0-9]+)*)$/.exec(raw);
    if (semantic) return semantic[0] === raw && semantic[1] === context.board && semantic[2] === context.thread
      && postId(semantic[2]) === semantic[2] && semantic[3].length <= 49;
    return /^\/[a-z0-9]{1,10}\/(?:post\/[1-9][0-9]{0,18}|thread\/[1-9][0-9]{0,18}(?:#p[1-9][0-9]{0,18}|\?quote=[1-9][0-9]{0,18}#reply)?|catalog(?:#s=(?:[a-z0-9+\-]|%2F|%2C)+)?|)$/.test(raw);
  }
  const url = new URL(raw);
  return ['http:', 'https:'].includes(url.protocol) && !url.username && !url.password && !/[\u0000-\u0020\u007f]/.test(raw);
}
// Validate again before DOM construction. Only these inert tags/attributes can
// cross from the worker's data tree into the browser, even with a bad response.
export function validatePostTree(tree, context, no, budget = { nodes: 0 }, limits = UPDATER_LIMITS) {
  budget.chars ??= 0;
  const charge = text => { budget.chars += text.length; require(budget.chars <= limits.bytes); };
  const ids = new Set(), expectedIds = new Set(['pc', 'sa', 'p', 'pi', 'pim', 'm', 'f', 'fT', 'delete', 'report'].map(prefix => prefix + no));
  function visit(node, depth, form = null, comment = false) {
    require(++budget.nodes <= limits.nodes && depth <= limits.depth);
    if (typeof node === 'string') { charge(node); return; }
    exactKeys(node, ['tag', 'attrs', 'children']);
    require((Object.hasOwn(attributes, node.tag) || isWordfilterMarkupTag(node.tag)) && Array.isArray(node.children));
    require(node.attrs && typeof node.attrs === 'object' && !Array.isArray(node.attrs));
    if (comment && isWordfilterMarkup(node.tag, node.attrs)) {
      for (const [key, value] of Object.entries(node.attrs)) { charge(key); charge(value); }
      for (const child of node.children) visit(child, depth + 1, form, true);
      return;
    }
    require(Object.hasOwn(attributes, node.tag));
    for (const [key, value] of Object.entries(node.attrs)) {
      // Rust permits 64 KiB of comment UTF-8; URL percent encoding can triple
      // that size. Preserve those valid links within the aggregate wire budget.
      require(attributes[node.tag].includes(key) && typeof value === 'string' && value.length <= (key === 'href' ? 192000 : 4096));
      charge(value);
      if (key === 'class') require(value.split(' ').every(token => classes.has(token) || isPostFlagToken(token) || isCapcodeToken(token)));
      if (key === 'id') { require(expectedIds.has(value) && !ids.has(value)); ids.add(value); }
      if (key === 'href') require(postLinkUrl(value, context));
      if (key === 'src') require(postMediaUrl(value, context) || postIdentityUrl(value) || postFileAssetUrl(value));
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
      if (key.startsWith('data-')) {
        if (key === 'data-custom-spoiler') {
          require(depth === 0 && node.tag === 'article' && /^(?:[1-9]|[1-5][0-9]|6[0-4])$/.test(value));
          continue;
        }
        if (key === 'data-utc') {
          require(node.tag === 'span' && node.attrs.class === 'dateTime postNum' && /^-?(?:0|[1-9][0-9]{0,11})$/.test(value));
          continue;
        }
        require(node.tag === 'div' && node.attrs.class === 'file' && node.attrs['data-image-spoiler'] === 'true');
        if (key === 'data-image-filename') {
          require(value.length > 0 && value.length <= 255 && !/[\u0000-\u001f\u007f-\u009f]/.test(value)
            && new TextEncoder().encode(value).length <= 255);
        } else if (['data-thumbnail-width', 'data-thumbnail-height'].includes(key)) {
          require(/^[1-9][0-9]{0,3}$/.test(value) && Number(value) <= 1024);
        } else require(value === 'true');
      }
    }
    const mobileLabel = node.tag === 'span' && ['name', 'subject'].includes(node.attrs.class) && Object.hasOwn(node.attrs, 'title');
    const fileTitle = node.tag === 'div' && node.attrs.class === 'fileText' && Object.hasOwn(node.attrs, 'title');
    if (fileTitle) require(Object.keys(node.attrs).sort().join(',') === 'class,id,title'
      && new TextEncoder().encode(node.attrs.title).length <= 255 && !/[\u0000-\u001f\u007f-\u009f]/.test(node.attrs.title));
    if (mobileLabel) require(Object.keys(node.attrs).sort().join(',') === 'class,title'
      && new TextEncoder().encode(node.attrs.title).length <= (node.attrs.class === 'name' ? 255 : 1020)
      && !/[\u0000-\u001f\u007f-\u009f]/.test(node.attrs.title));
    if ((Object.hasOwn(node.attrs, 'title') && !['strong', 'img', 'a'].includes(node.tag) && !mobileLabel && !fileTitle)
      || (node.attrs.class || '').split(' ').some(isPostFlagToken)) {
      require(node.tag === 'span' && isPostFlagClass(node.attrs.class || '')
        && Object.keys(node.attrs).sort().join(',') === 'class,title'
        && node.children.length === 0 && typeof node.attrs.title === 'string'
        && new TextEncoder().encode(node.attrs.title).length >= 1
        && new TextEncoder().encode(node.attrs.title).length <= 100
        && !/[\u0000-\u001f\u007f-\u009f]/.test(node.attrs.title));
    }
    if (node.tag === 'form') {
      require(form === null && node.attrs.method === 'post' && ['delete', 'report'].some(action => node.attrs.action === `/${context.board}/${action}`));
      form = node.attrs.action;
    }
    if (node.tag === 'input') {
      const a = node.attrs;
      require(form !== null);
      require((a.type === 'hidden' && a.name === 'no' && a.value === no && !a.id)
        || (form.endsWith('/delete') && a.type === 'hidden' && a.name === 'password' && a.id === `delete${no}`
          && Object.keys(a).sort().join(',') === 'id,name,type')
        || (form.endsWith('/delete') && a.type === 'password' && a.name === 'password' && a.id === `delete${no}` && a.value === undefined)
        || (form.endsWith('/delete') && a.type === 'checkbox' && a.name === 'file_only' && a.value === 'true' && !a.id)
        || (form.endsWith('/report') && !a.type && a.name === 'reason' && a.id === `report${no}` && a.value === undefined));
    }
    if (node.tag === 'button') require(form !== null);
    if (node.tag === 'pre') require(node.attrs.class === 'prettyprint');
    if ((node.attrs.class || '').split(' ').some(value => ['mu-s', 'mu-i', 'mu-r', 'mu-g', 'mu-b'].includes(value))) {
      require(node.tag === 'span' && ['mu-s', 'mu-i', 'mu-r', 'mu-g', 'mu-b'].includes(node.attrs.class));
    }
    const fortuneTokens = (node.attrs.class || '').split(' ').filter(value => value === 'fortune' || /^fortune-(?:[0-9]|1[0-2])$/.test(value));
    if (fortuneTokens.length) {
      require(node.tag === 'span' && /^fortune fortune-(?:[0-9]|1[0-2])$/.test(node.attrs.class));
    }
    if (node.tag === 'img') require(typeof node.attrs.src === 'string' && typeof node.attrs.alt === 'string');
    if (node.tag === 'a') {
      require(typeof node.attrs.href === 'string');
      if (!node.attrs.href.startsWith('/')) require(['noopener noreferrer', 'nofollow noreferrer noopener'].includes(node.attrs.rel));
    }
    const message = node.tag === 'blockquote' && node.attrs.class === 'postMessage' && node.attrs.id === `m${no}`;
    for (const child of node.children) visit(child, depth + 1, form, comment || message);
  }
  visit(tree, 0);
  validateCapcodeTree(tree, no);
  validatePostNumbers(tree, context, no);
  validateFilePresentation(tree, context, no);
  require(tree.tag === 'article' && tree.attrs.id === `pc${no}`
    && tree.attrs.class === `postContainer ${no === context.thread ? 'opContainer' : 'replyContainer'}`);
  for (const prefix of ['pc', 'p', 'pi', 'm']) require(ids.has(prefix + no));
  return tree;
}

export function parsePostRecipe(html, context, no, budget, limits) {
  budget.created ??= 0;
  const treeAdapter = { ...defaultTreeAdapter, createElement(...args) {
    require(++budget.created <= limits.nodes); return defaultTreeAdapter.createElement(...args);
  } };
  const fragment = parseFragment(html, { treeAdapter, scriptingEnabled: true });
  function recipe(node, depth) {
    require(depth <= limits.depth);
    if (node.nodeName === '#text') return node.value;
    require(node.namespaceURI === 'http://www.w3.org/1999/xhtml'
      && (Object.hasOwn(attributes, node.tagName) || isWordfilterMarkupTag(node.tagName)));
    return { tag: node.tagName, attrs: Object.fromEntries(node.attrs.map(a => {
      require(!a.namespace && !a.prefix); return [a.name, a.value];
    })), children: node.childNodes.map(child => recipe(child, depth + 1)) };
  }
  const roots = fragment.childNodes.filter(node => node.nodeName !== '#text' || node.value.trim() !== '');
  require(roots.length === 1);
  return validatePostTree(recipe(roots[0], 0), context, no, budget, limits);
}

const previewId = value => typeof value === 'string' && !/\D/.test(value) && postId(value) === value;
export function previewContext({ origin, board, post, mediaOrigin = '', thread = null }) {
  require(typeof board === 'string' && !/[^a-z0-9]/.test(board));
  require(previewId(post));
  require(thread === null || (previewId(thread) && BigInt(thread) <= BigInt(post)));
  const context = updaterContext({ origin, board, thread: thread ?? post, mediaOrigin });
  return { ...context, post, thread };
}

export function previewUrl(input) {
  const { origin, board, post } = previewContext(input);
  return `${origin}/_watch/${board}/post/${post}`;
}

export function validatePreviewMetadata(snapshot, input, parsed = false) {
  const context = previewContext(input);
  exactKeys(snapshot, ['version', 'board', 'thread', 'post']);
  require(snapshot.version === 1 && snapshot.board === context.board
    && previewId(snapshot.thread)
    && BigInt(snapshot.thread) <= BigInt(context.post)
    && (context.thread === null || context.thread === snapshot.thread));
  exactKeys(snapshot.post, ['no', 'file_deleted', parsed ? 'tree' : 'html']);
  require(snapshot.post.no === context.post && typeof snapshot.post.file_deleted === 'boolean');
  if (!parsed) require(typeof snapshot.post.html === 'string');
  return updaterContext({ ...context, thread: snapshot.thread });
}

export function parseQuotePreviewSnapshot(raw, input) {
  try {
    require(typeof raw === 'string' && raw.length <= PREVIEW_LIMITS.bytes
      && new TextEncoder().encode(raw).length <= PREVIEW_LIMITS.bytes);
    const snapshot = JSON.parse(raw), context = validatePreviewMetadata(snapshot, input);
    const { no, file_deleted, html } = snapshot.post;
    const tree = parsePostRecipe(html, context, no, { nodes: 0 }, PREVIEW_LIMITS);
    return { status: 'ok', snapshot: { ...snapshot, post: { no, file_deleted, tree } } };
  } catch { return { status: 'invalid-preview' }; }
}

// Runs only in a disposable worker. parse5 creates data, never live DOM, so a
// rejected img, SVG, style or script cannot initiate a request while parsing.
export function parseUpdaterSnapshot(raw, inputContext) {
  try {
    const context = updaterContext(inputContext);
    require(typeof raw === 'string' && raw.length <= UPDATER_LIMITS.bytes && new TextEncoder().encode(raw).length <= UPDATER_LIMITS.bytes);
    const snapshot = JSON.parse(raw);
    validateSnapshotMetadata(snapshot, context);
    let previous = 0n;
    const budget = { nodes: 0 };
    const posts = snapshot.posts.map((post, index) => {
      exactKeys(post, ['no', 'file_deleted', 'html']);
      require(postId(post.no) === post.no && BigInt(post.no) > previous && (index !== 0 || post.no === context.thread));
      previous = BigInt(post.no);
      require(typeof post.file_deleted === 'boolean' && typeof post.html === 'string');
      const tree = parsePostRecipe(post.html, context, post.no, budget, UPDATER_LIMITS);
      return { no: post.no, file_deleted: post.file_deleted, tree };
    });
    return { status: 'ok', snapshot: { ...snapshot, posts } };
  } catch { return { status: 'invalid-snapshot' }; }
}

export function boardPageContext({ origin, board, page, mediaOrigin = '' }) {
  const context = updaterContext({ origin, board, thread: '1', mediaOrigin });
  require(Number.isInteger(page) && page >= 0 && page <= 999);
  return { origin: context.origin, board: context.board, page, mediaOrigin: context.mediaOrigin };
}

export function validateBoardPageSnapshot(snapshot, input, parsed = true) {
  const context = boardPageContext(input);
  exactKeys(snapshot, ['version', 'board', 'page', 'next_page', 'threads']);
  require(snapshot.version === 1 && snapshot.board === context.board && snapshot.page === context.page
    && (snapshot.next_page === null || (context.page < 999 && snapshot.next_page === context.page + 1))
    && Array.isArray(snapshot.threads) && snapshot.threads.length <= 20
    && (snapshot.threads.length > 0 || snapshot.next_page === null));
  const ids = new Set(), threads = new Set(), budget = { nodes: 0 };
  for (const thread of snapshot.threads) {
    exactKeys(thread, ['thread', 'closed', 'sticky', 'archived', 'replies', 'images', 'omitted', 'posts']);
    require(typeof thread.thread === 'string' && postId(thread.thread) === thread.thread && !threads.has(thread.thread));
    threads.add(thread.thread);
    require(typeof thread.closed === 'boolean' && typeof thread.sticky === 'boolean' && thread.archived === false
      && Number.isInteger(thread.replies) && thread.replies >= 0 && thread.replies <= 1000
      && Number.isInteger(thread.images) && thread.images >= 0 && thread.images <= thread.replies
      && Array.isArray(thread.posts) && thread.posts.length === Math.min(4, thread.replies + 1)
      && thread.omitted === thread.replies - (thread.posts.length - 1));
    const postContext = updaterContext({ ...context, thread: thread.thread });
    let previous = 0n;
    for (const [index, post] of thread.posts.entries()) {
      exactKeys(post, ['no', 'file_deleted', parsed ? 'tree' : 'html']);
      require(typeof post.no === 'string' && postId(post.no) === post.no && !ids.has(post.no)
        && BigInt(post.no) > previous && (index !== 0 || post.no === thread.thread)
        && typeof post.file_deleted === 'boolean');
      ids.add(post.no); previous = BigInt(post.no);
      if (parsed) validatePostTree(post.tree, postContext, post.no, budget);
      else require(typeof post.html === 'string');
    }
  }
  return snapshot;
}

export function parseBoardPageSnapshot(raw, input) {
  try {
    const context = boardPageContext(input);
    require(typeof raw === 'string' && raw.length <= UPDATER_LIMITS.bytes
      && new TextEncoder().encode(raw).length <= UPDATER_LIMITS.bytes);
    const snapshot = JSON.parse(raw);
    validateBoardPageSnapshot(snapshot, context, false);
    const budget = { nodes: 0 };
    const threads = snapshot.threads.map(thread => ({ ...thread, posts: thread.posts.map(post => ({
      no: post.no, file_deleted: post.file_deleted,
      tree: parsePostRecipe(post.html, { ...context, thread: thread.thread }, post.no, budget, UPDATER_LIMITS),
    })) }));
    return { status: 'ok', snapshot: { ...snapshot, threads } };
  } catch { return { status: 'invalid-snapshot' }; }
}
