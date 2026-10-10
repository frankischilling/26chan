import { sourceSpoilerPath } from './native-spoilers.js';
import { quotePostId } from './native-quote-identity.js';
import { isSpoilerAssetPath } from './native-spoiler-assets.js';
import { postId } from '../static/thread-watcher-core.v1.js';
import { FILTER_LIMITS } from './native-filter-limits.js';
import { isPostFlagToken } from './native-post-flags.js';
import { isCapcodeToken, postIdentityUrl } from './native-capcodes.js';
import { postFileAssetUrl } from './native-file-presentation.js';
import { PREVIEW_LIMITS, previewContext, updaterContext, validatePostTree, postLinkUrl, postMediaUrl, quoteTarget } from './native-updater-snapshot.js';
import { NativeQuotePreviewTransport, checkedQuotePreview } from './native-quote-preview-transport.js';
import { isCommentElement, isWordfilterMarkup, isWordfilterMarkupTag } from './native-wordfilter-markup.js';

export const mobileQuoteDevice = userAgent => typeof userAgent === 'string'
  && /Mobile|Android|Dolfin|Opera Mobi|PlayStation Vita|Nintendo DS/.test(userAgent);

export { quoteTarget } from './native-updater-snapshot.js';

export function quotePreviewPosition(link, size, viewport, mobile = false) {
  const width = Math.max(1, viewport.width), height = Math.max(1, viewport.height);
  const leftSide = width - link.right < Math.floor(width * 0.3);
  const x = mobile ? (leftSide ? link.right - size.width : link.left)
    : (leftSide ? link.left - size.width - 5 : link.right + 5);
  const y = mobile ? link.bottom : link.top + link.height / 2 - size.height / 2;
  return { left: (viewport.x || 0) + Math.max(0, Math.min(x, Math.max(0, width - size.width))),
    top: (viewport.y || 0) + Math.max(0, Math.min(y, Math.max(0, height - size.height))) };
}

const localTags = {
  article: ['class', 'id', 'data-custom-spoiler'],
  div: ['class', 'id', 'title', 'data-image-spoiler', 'data-image-filename', 'data-thumbnail-width', 'data-thumbnail-height', 'data-thumbnail-legacy'],
  span: ['class', 'tabindex', 'aria-label', 'title', 'data-utc'],
  strong: ['class', 'title'], time: ['datetime'], a: ['class', 'href', 'target', 'rel', 'title'], blockquote: ['class', 'id'],
  br: [], wbr: [], b: [], s: [], small: [], pre: ['class'], p: ['class'], img: ['class', 'src', 'srcset', 'alt', 'title', 'width', 'height', 'loading'],
};
const localClasses = new Set(['postContainer', 'opContainer', 'replyContainer', 'post', 'op', 'reply',
  'postInfo', 'postInfoM', 'mobile', 'dateTime', 'subject', 'name', 'postertrip', 'posteruid', 'hand', 'postNum', 'file', 'fileText', 'mFileInfo', 'fileThumb', 'imgspoiler', 'fileDeleted', 'fileDeletedRes', 'postMessage',
  'quote', 'quotelink', 'spoiler', 'sjis', 'mu-s', 'mu-i', 'mu-r', 'mu-g', 'mu-b', 'prettyprint',
  'fortune', 'fortune-0', 'fortune-1', 'fortune-2', 'fortune-3', 'fortune-4', 'fortune-5', 'fortune-6',
  'fortune-7', 'fortune-8', 'fortune-9', 'fortune-10', 'fortune-11', 'fortune-12']);
const controls = '.postActions,.postMenuBtn,.extButton,.extControls,.filter-preview,.quoteLink,.sideArrows,.backlink,[data-drawing-edit-wrap]';

// Read a bounded inert recipe from the original DOM. Never clone an element with
// an unchecked src/srcset, and never read a form control's attributes or value.
export function localQuoteTree(article, context, no, projection) {
  let nodes = 0, bytes = 0;
  const encoder = new TextEncoder();
  const charge = value => {
    if (value.length > PREVIEW_LIMITS.bytes) throw new RangeError('preview-size');
    bytes += encoder.encode(value).length;
    if (bytes > PREVIEW_LIMITS.bytes) throw new RangeError('preview-size');
  };
  const ids = new Set(['pc', 'p', 'pi', 'pim', 'm', 'f', 'fT'].map(prefix => prefix + no));
  function read(node, depth, comment = false) {
    if (projection?.has(node)) return [];
    if (++nodes > PREVIEW_LIMITS.nodes || depth > PREVIEW_LIMITS.depth) throw new RangeError('preview-nodes');
    if (node.nodeType === 3) {
      const text = projection?.sourceText?.(node) ?? node.data;
      charge(text); return [text];
    }
    if (node.nodeType !== 1 || node.namespaceURI !== 'http://www.w3.org/1999/xhtml') return [];
    const tag = node.localName;
    if ((!Object.hasOwn(localTags, tag) && !isWordfilterMarkupTag(tag)) || node.matches(controls)) return [];
    charge(tag + ' '.repeat(12));
    const originalAttrs = Object.fromEntries(Array.from(projection?.attributes(node) ?? node.attributes,
      ({ name, value }) => [name, value]));
    if (comment && isWordfilterMarkup(tag, originalAttrs)) {
      for (const [key, value] of Object.entries(originalAttrs)) { charge(key); charge(value); }
      if (node.childNodes.length + nodes > PREVIEW_LIMITS.nodes) throw new RangeError('preview-nodes');
      return [{ tag, attrs: originalAttrs, children: Array.from(node.childNodes).flatMap(child => read(child, depth + 1, true)) }];
    }
    if (!Object.hasOwn(localTags, tag)) return [];
    const attrs = {};
    for (const key of localTags[tag]) {
      if (tag === 'div' && key.startsWith('data-') && (!(originalAttrs.class || '').split(' ').includes('file')
        || !isSpoilerAssetPath(node.querySelector('a.fileThumb.imgspoiler img')?.getAttribute('src')))) continue;
      if (key === 'title' && tag === 'span' && !Array.from(node.classList).some(isPostFlagToken)
        && !(['name', 'subject'].includes(node.className) && node.closest('.postInfoM'))) continue;
      if (key === 'title' && tag === 'a' && !node.parentElement?.matches('.postInfo > span.postNum,.postInfoM > span.dateTime.postNum,.file > .fileText')) continue;
      if (key === 'title' && tag === 'div' && node.className !== 'fileText') continue;
      if (tag === 'img' && ['class', 'srcset', 'title'].includes(key) && !node.classList.contains('identityIcon') && !node.classList.contains('fileDeletedRes')) continue;
      const value = node.getAttribute(key);
      if (value !== null) { charge(key); charge(value); attrs[key] = value; }
    }
    if (attrs.class !== undefined) attrs.class = attrs.class.split(/\s+/).filter(value => localClasses.has(value) || isPostFlagToken(value) || isCapcodeToken(value)
      || (comment && tag === 'span' && originalAttrs.class === 'deadlink' && value === 'deadlink')).join(' ');
    if (!attrs.class) delete attrs.class;
    if (!ids.has(attrs.id)) delete attrs.id;
    if (tag === 'img') {
      if (!attrs.src || (!postMediaUrl(attrs.src, context) && !postIdentityUrl(attrs.src) && !postFileAssetUrl(attrs.src))) return [];
      attrs.alt ??= '';
      if (!postIdentityUrl(attrs.src)) attrs.loading = 'lazy';
      for (const key of ['width', 'height']) if (!/^[1-9][0-9]{0,3}$/.test(attrs[key] ?? '')) delete attrs[key];
    }
    if (Array.from(node.childNodes).filter(child => !projection?.has(child)).length + nodes > PREVIEW_LIMITS.nodes) throw new RangeError('preview-nodes');
    const message = tag === 'blockquote' && node.className === 'postMessage' && node.id === `m${no}`;
    const children = Array.from(node.childNodes).flatMap(child => read(child, depth + 1, comment || message));
    if (tag === 'a') {
      if (attrs.href?.startsWith('#p')) {
        const ref = quoteTarget(attrs.href, context);
        if (ref?.thread) attrs.href = `/${ref.board}/thread/${ref.thread}#p${ref.post}`;
      }
      let safe = false;
      try { safe = typeof attrs.href === 'string' && postLinkUrl(attrs.href, context); } catch { /* Keep its label only. */ }
      if (!safe || node.hasAttribute('data-native-linkified')) return children;
      if (!attrs.href.startsWith('/')) attrs.rel = 'nofollow noreferrer noopener';
      else delete attrs.rel;
      if (attrs.target !== '_blank') delete attrs.target;
    }
    return [{ tag, attrs, children }];
  }
  const [tree] = read(article, 0);
  return validatePostTree(tree, context, no, { nodes: 0 }, PREVIEW_LIMITS);
}

// The prepared plan contains only ID-free, inert post content. Inline admission
// can charge its complete node/UTF-16 text-and-attribute budget before build().
export function prepareQuotePost(tree, context, no) {
  validatePostTree(tree, context, no, { nodes: 0 }, PREVIEW_LIMITS);
  const post = tree.children.find(node => typeof node !== 'string' && node.tag === 'div'
    && node.attrs.id === `p${no}` && node.attrs.class?.split(' ').includes('post'));
  if (!post) throw new TypeError('invalid-preview');
  let nodes = 0, characters = 0;
  const quotes = [];
  function clean(node) {
    if (typeof node === 'string') { nodes++; characters += node.length; return node; }
    if ((!Object.hasOwn(localTags, node.tag) && !isWordfilterMarkupTag(node.tag))
      || node.attrs.class?.split(' ').some(value => ['postActions', 'sideArrows'].includes(value))) return null;
    nodes++;
    const attrs = Object.fromEntries(Object.entries(node.attrs).filter(([key]) => key !== 'id'));
    // A fetched preview can belong to a different thread than the live page.
    // Bind its bare fragments before copying them into that page's DOM.
    if (node.tag === 'a' && attrs.href?.startsWith('#p')) {
      const ref = quoteTarget(attrs.href, context);
      if (!ref?.thread) throw new TypeError('invalid-preview');
      attrs.href = `/${ref.board}/thread/${ref.thread}#p${ref.post}`;
    }
    for (const [key, value] of Object.entries(attrs)) characters += key.length + value.length;
    if (node.tag === 'a' && attrs.class?.split(' ').includes('quotelink')) quotes.push(attrs.href);
    return { tag: node.tag, attrs, children: node.children.map(clean).filter(child => child !== null) };
  }
  const safe = clean(post);
  if (nodes > PREVIEW_LIMITS.nodes || characters > PREVIEW_LIMITS.bytes) throw new RangeError('preview-size');
  return { nodes, characters, quotes, build(document) {
    const path = sourceSpoilerPath(document, context.board, Number(tree.attrs['data-custom-spoiler'] ?? 0));
    function build(node) {
      if (typeof node === 'string') return document.createTextNode(node);
      const element = document.createElement(node.tag);
      for (const [key, value] of Object.entries(node.attrs)) element.setAttribute(key, key === 'src' && node.tag === 'img' && isSpoilerAssetPath(value) ? path : value);
      for (const child of node.children) element.append(build(child));
      return element;
    }
    return build(safe);
  } };
}

const escapedSize = text => text.replace(/[&<>"\u00a0]/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', '\u00a0': '&nbsp;' })[ch]).length;
// Conservatively measure serialized HTML and decoded filter text without making
// a clone or parsing HTML on the UI thread. BR contributes the source newline.
function messageBudget(message, projection) {
  let nodes = 0, html = 0, text = 0;
  function visit(parent, depth) {
    if (depth > PREVIEW_LIMITS.depth
      || Array.from(parent.childNodes).filter(child => !projection?.has(child)).length + nodes > PREVIEW_LIMITS.nodes) throw new RangeError('quote-nodes');
    for (const child of parent.childNodes) {
      if (projection?.has(child)) continue;
      if (++nodes > PREVIEW_LIMITS.nodes) throw new RangeError('quote-nodes');
      if (child.nodeType === 3) {
        const value = projection?.sourceText(child) ?? child.data;
        if (value.length > FILTER_LIMITS.html) throw new RangeError('quote-text');
        html += escapedSize(value); text += value.length;
      } else if (child.nodeType === 1 && isCommentElement(child, projection?.attributes(child) ?? child.attributes)) {
        const leaf = child.tagName === 'BR' || child.tagName === 'WBR';
        html += leaf ? child.localName.length + 2 : child.localName.length * 2 + 5;
        for (const { name, value } of projection?.attributes(child) ?? child.attributes) {
          if (value.length > FILTER_LIMITS.html) throw new RangeError('quote-attribute');
          html += name.length + escapedSize(value) + 4;
        }
        if (child.tagName === 'BR') text++;
        visit(child, depth + 1);
      } else throw new TypeError('quote-node');
      if (html > FILTER_LIMITS.html || text > FILTER_LIMITS.field) throw new RangeError('quote-budget');
    }
  }
  visit(message, 0);
  return { html, text };
}

export function mountNativeQuotePreview({ root, board, thread = null, mediaOrigin = '', settings,
  origin = globalThis.location?.origin, userAgent = globalThis.navigator?.userAgent,
  transport = new NativeQuotePreviewTransport({ origin, mediaOrigin }), decorate, companion, decoratePreview,
  projection, arbitrateClick, inlineHoverEligible, quoteContext, registerMath } = {}) {
  if (!root || typeof settings !== 'function') return null;
  previewContext({ origin, mediaOrigin, board, post: thread ?? '1', thread });
  const document = root.ownerDocument, window = document.defaultView;
  const page = { origin, board, thread }, mobile = mobileQuoteDevice(userAgent);
  const companions = new Map();
  const ownedCompanion = link => {
    const node = companion?.(link);
    return node?.parentNode === link.parentNode && link.nextSibling === node
      && node?.matches?.('a.quoteLink') && node.getAttribute('href') === link.getAttribute('href') ? node : null;
  };
  const hasCompanion = link => companions.has(link) || ownedCompanion(link);
  let active = null, disposed = false, suspended = false, scheduled = false;
  function enabled() {
    if (disposed || suspended) return false;
    let value = {};
    try { value = settings() ?? {}; } catch { /* Default-on also works without storage. */ }
    return value.quotePreview !== false && value.disableAll !== true;
  }
  const hidden = post => !!post.closest('.post-hidden,.native-thread-hidden,[hidden]');
  const target = anchor => quoteTarget(anchor.getAttribute('href'), quoteContext?.(anchor) ?? page);
  function candidate(node) {
    const anchor = node?.closest?.('a.quotelink');
    return anchor && root.contains(anchor) && !anchor.classList.contains('linkfade')
      && (!anchor.classList.contains('deadlink') || active?.dead === anchor) && target(anchor) ? anchor : null;
  }
  function clear() {
    const current = active; active = null;
    if (!current) return;
    clearTimeout(current.timer); current.controller.abort(); transport.cancel();
    current.resize?.disconnect(); current.popup?.remove();
    if (current.mark) {
      const { node, name, family, previous } = current.mark;
      node.classList.remove(name);
      if (family && node.getAttribute('data-quote-anti') === family) {
        if (previous === null) node.removeAttribute('data-quote-anti');
        else node.setAttribute('data-quote-anti', previous);
      }
    }
    if (current.dead) current.dead.classList.remove('deadlink');
    if (current.link.style.cursor === 'wait') current.link.style.cursor = current.cursor;
  }
  function current(value) {
    return active === value && enabled() && root.contains(value.link)
      && !hidden(value.link) && value.link.classList.contains('quotelink') && !value.link.classList.contains('linkfade')
      && value.link.getAttribute('href') === value.href && !value.controller.signal.aborted;
  }
  function position() {
    if (!active?.popup) return;
    if (!current(active)) { clear(); return; }
    const popup = active.popup, viewport = document.documentElement;
    popup.style.maxWidth = `${Math.max(1, viewport.clientWidth - 10)}px`;
    popup.style.maxHeight = `${Math.max(1, viewport.clientHeight - 10)}px`;
    const point = quotePreviewPosition(active.link.getBoundingClientRect(), popup.getBoundingClientRect(),
      { width: viewport.clientWidth, height: viewport.clientHeight, x: window.scrollX, y: window.scrollY }, mobile);
    popup.style.left = `${point.left}px`; popup.style.top = `${point.top}px`;
  }
  function show(value, tree, context) {
    if (!current(value)) return;
    // This check precedes every createElement and every resource assignment.
    const budget = { nodes: 0 };
    const no = quotePostId(value.ref.post);
    validatePostTree(tree, context, no, budget, PREVIEW_LIMITS);
    if (document.getElementById('quote-preview')) throw new TypeError('preview-exists');
    const popup = prepareQuotePost(tree, context, no).build(document);
    popup.id = 'quote-preview'; popup.classList.add('preview');
    if (!value.link.closest('.backlink')) popup.classList.add('reveal-spoilers');
    if (context.board === board && window.location.hash === `#p${no}`) popup.classList.add('highlight');
    popup.style.pointerEvents = mobile ? 'auto' : 'none';
    if (value.target) decoratePreview?.(popup, value.target, value.link,
      { nodes: PREVIEW_LIMITS.nodes - budget.nodes, characters: PREVIEW_LIMITS.bytes - budget.chars });
    registerMath?.(popup, context);
    value.popup = popup; document.body.append(popup);
    decorate?.(); position();
    popup.addEventListener('load', position, true);
    if (typeof window.ResizeObserver === 'function') { value.resize = new window.ResizeObserver(position); value.resize.observe(popup); }
  }
  async function remote(value) {
    const result = await transport.load(value.ref, { signal: value.controller.signal });
    if (!current(value)) return;
    if (result.status === 'cooldown') {
      value.timer = setTimeout(() => {
        if (current(value)) void remote(value).catch(() => { if (active === value) clear(); });
      }, Math.min(PREVIEW_LIMITS.intervalMs, Math.max(1, result.retryAfter)));
      return;
    }
    if (value.link.style.cursor === 'wait') value.link.style.cursor = value.cursor;
    if (result.status === 'http-error' && result.httpStatus === 404) {
      value.dead = value.link; value.link.classList.add('deadlink'); return;
    }
    const checked = checkedQuotePreview(result, { ...value.ref, origin, mediaOrigin });
    if (checked.status === 'ok') {
      try { show(value, checked.snapshot.post.tree, checked.context); } catch { clear(); }
    }
  }
  function begin(link, clickOwned = false) {
    if (!enabled() || hidden(link) || (mobile && !hasCompanion(link))) return;
    if (active?.link === link && active.href === link.getAttribute('href')) {
      // A tap follows compatibility mouseover, so promote its existing preview.
      if (clickOwned) active.clickOwned = true;
      return;
    }
    clear();
    const ref = target(link);
    if (!ref) return;
    const value = { link, href: link.getAttribute('href'), ref, clickOwned,
      cursor: link.style.cursor, controller: new AbortController() };
    active = value;
    const post = ref.board === board ? document.getElementById(`p${ref.post}`) : null;
    const article = post?.closest('.postContainer'), section = post?.closest('.thread');
    const parent = section && postId(section.id.slice(1));
    if (post && root.contains(post) && article?.id === `pc${ref.post}` && parent
      && BigInt(parent) <= BigInt(ref.post) && (ref.thread === null || quotePostId(ref.thread) === parent)) {
      value.target = post;
      const rect = post.getBoundingClientRect();
      if (rect.top > 0 && rect.bottom < document.documentElement.clientHeight && post.getClientRects().length && !hidden(post)) {
        const name = post.classList.contains('highlight') || window.location.hash === `#${post.id}` ? 'highlight-anti' : 'highlight';
        if (!(name === 'highlight-anti' && post.classList.contains('op')) && !post.classList.contains(name)) {
          post.classList.add(name); value.mark = { node: post, name };
          if (name === 'highlight-anti') {
            const family = window.getComputedStyle(document.documentElement).getPropertyValue('--watcher-icon-family').trim();
            value.mark.family = ['futaba', 'burichan', 'tomorrow', 'photon'].includes(family) ? family : 'futaba';
            value.mark.previous = post.getAttribute('data-quote-anti');
            post.setAttribute('data-quote-anti', value.mark.family);
          }
        }
        return;
      }
      try {
        const context = updaterContext({ origin, board, thread: parent, mediaOrigin });
        show(value, localQuoteTree(article, context, ref.post, projection), context);
      } catch { clear(); }
      return;
    }
    link.style.cursor = 'wait';
    void remote(value).catch(() => { if (active === value) clear(); });
  }
  function removeCompanions(predicate = () => true) {
    for (const [link, companion] of companions) if (predicate(link, companion)) { companion.remove(); companions.delete(link); }
  }
  function decorateMessage(message) {
    if (projection?.within(message)) return;
    let budget, links;
    try {
      budget = messageBudget(message, projection);
      links = [...message.querySelectorAll('a.quotelink')].filter(link => !projection?.within(link)
        && target(link) && !companions.has(link) && !ownedCompanion(link));
      if (links.length + companions.size > PREVIEW_LIMITS.companions) return;
      for (const link of links) {
        // Exact owned markup: <a class="quoteLink" href="..."> #</a>.
        budget.html += 35 + escapedSize(link.getAttribute('href'));
        budget.text += 2;
      }
      if (budget.html > FILTER_LIMITS.html || budget.text > FILTER_LIMITS.field) throw new RangeError('quote-budget');
    } catch {
      removeCompanions(link => message.contains(link)); return;
    }
    for (const link of links) {
      const companion = document.createElement('a'); companion.className = 'quoteLink';
      companion.setAttribute('href', link.getAttribute('href')); companion.textContent = ' #';
      link.after(companion); companions.set(link, companion);
    }
  }
  function refresh() {
    if (active && (!current(active) || (active.target && !root.contains(active.target)) || (active.mark && hidden(active.mark.node)))) clear();
    if (!enabled() || !mobile) { removeCompanions(); return; }
    removeCompanions((link, companion) => !root.contains(link) || link.nextSibling !== companion
      || !link.classList.contains('quotelink') || !target(link) || companion.getAttribute('href') !== link.getAttribute('href'));
    const messages = root.querySelectorAll('.postMessage,.backlink');
    if (messages.length <= 20001) for (const message of messages) decorateMessage(message);
  }
  const observer = new window.MutationObserver(() => {
    if (scheduled || disposed || suspended) return;
    scheduled = true;
    queueMicrotask(() => { scheduled = false; if (!disposed && !suspended) refresh(); });
  });
  const observe = () => observer.observe(root, { childList: true, subtree: true, characterData: true,
    attributes: true, attributeFilter: ['href', 'class', 'hidden'] });
  const over = event => {
    const link = candidate(event.target);
    if (!link || link.contains(event.relatedTarget)) return;
    if (mobile && event.target === link && event.sourceCapabilities?.firesTouchEvents === true
      && typeof inlineHoverEligible === 'function') {
      try { if (inlineHoverEligible(link) === true) return; } catch { /* Preview remains the fallback. */ }
    }
    begin(link);
  };
  const out = event => {
    if (active && !active.clickOwned && active.link.contains(event.target) && !active.link.contains(event.relatedTarget)) clear();
  };
  function click(event) {
    const disposition = arbitrateClick?.(event) ?? 'passpreview';
    if (disposition !== 'passpreview') { clear(); return; }
    const link = candidate(event.target);
    if (mobile && enabled() && link === event.target && hasCompanion(link) && event.button === 0
      && !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey) { event.preventDefault(); begin(link, true); }
    else if (active && !active.popup?.contains(event.target) && !active.link.contains(event.target)) clear();
  }
  const storage = event => { if (event.key === null || event.key === '4chan-settings') refresh(); };
  const hide = event => {
    suspended = true; clear(); removeCompanions(); observer.disconnect();
    if (!event.persisted) disconnect();
  };
  const restore = event => { if (event.persisted && !disposed) { suspended = false; observe(); refresh(); } };
  function disconnect() {
    if (disposed) return;
    disposed = true; clear(); removeCompanions(); observer.disconnect();
    root.removeEventListener('mouseover', over); root.removeEventListener('mouseout', out);
    document.removeEventListener('click', click);
    document.removeEventListener('4chanSettingsSaved', refresh);
    window.removeEventListener('storage', storage); window.removeEventListener('resize', position);
    window.removeEventListener('scroll', position, true);
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', restore);
  }
  root.addEventListener('mouseover', over); root.addEventListener('mouseout', out);
  document.addEventListener('click', click);
  document.addEventListener('4chanSettingsSaved', refresh);
  window.addEventListener('storage', storage); window.addEventListener('resize', position);
  window.addEventListener('scroll', position, { passive: true, capture: true });
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', restore);
  observe(); refresh();
  return { refresh, clear, disconnect, companion: link => companions.get(link) ?? ownedCompanion(link) };
}
