import { postId } from '../static/thread-watcher-core.v1.js';

// Characters count UTF-16 code units in copied text and attribute names/values.
// Pending placeholders reserve their complete ready/error status markup.
export const INLINE_LIMITS = Object.freeze({ open: 16, pending: 8, depth: 8,
  nodes: 32768, characters: 524288, quotes: 512, pendingMs: 15000 });
const placeholder = Object.freeze({ nodes: 2, characters: 256 });
const validId = value => typeof value === 'string' && postId(value) === value && !/\D/.test(value);
// Post identity is board-wide; a thread alias cannot turn a self/ancestor
// reference into a new post. Resolution and worker results still bind threads.
const samePost = (a, b) => a.board === b.board && a.post === b.post;

// Shared validation/build helpers and the projection registry are injected from
// the page's single instances. This module imports no parser or network code.
export function mountNativeInlineQuotes({ root, board, thread = null, mediaOrigin = '', settings,
  origin = globalThis.location?.origin, mobileDevice = false, archive = false,
  projection, quoteTarget, localQuoteTree, prepareQuotePost, checkedQuotePreview, transport,
  companion: externalCompanion, backlinkOwner, prepareBacklinks,
  navigate, limits = {} } = {}) {
  if (!root?.matches('.board') || typeof settings !== 'function' || !projection
    || [quoteTarget, localQuoteTree, prepareQuotePost, checkedQuotePreview].some(fn => typeof fn !== 'function')
    || typeof transport?.load !== 'function' || !/^[a-z0-9]{1,10}$/.test(board ?? '')
    || /[^a-z0-9]/.test(board) || (thread !== null && !validId(thread))) return null;
  try { if (new URL(origin).origin !== origin || !/^https?:\/\//.test(origin)) return null; }
  catch { return null; }
  const bounds = { ...INLINE_LIMITS };
  for (const [key, value] of Object.entries(limits)) {
    if (!Object.hasOwn(bounds, key) || !Number.isInteger(value) || value < 1 || value > bounds[key]) throw new RangeError('inline-limits');
    bounds[key] = value;
  }
  const document = root.ownerDocument, window = document.defaultView;
  navigate ??= href => window.location.assign(href);
  const page = { origin, board, thread }, identity = {};
  const entries = new Map(), hidden = new Map(), companions = new WeakMap();
  let queue = [], activeTask = null, wake = null, scheduled = false;
  let disposed = false, suspended = false, nodes = 0, characters = 0, pending = 0;

  function enabled() {
    let config = {};
    try { config = settings() ?? {}; } catch { /* Inline expansion defaults off. */ }
    return !disposed && !suspended && !archive && config.inlineQuotes === true && config.disableAll !== true;
  }
  function canonical(post) {
    if (!post || projection.within(post)) return null;
    const article = post.parentElement, section = article?.parentElement;
    const no = post.id?.slice(1), parent = section?.id?.slice(1);
    if (!post.matches('.post') || post.id !== `p${no}` || !validId(no) || !validId(parent)
      || BigInt(parent) > BigInt(no) || (thread !== null && thread !== parent)
      || !article?.matches('.postContainer') || article.id !== `pc${no}`
      || !section?.matches('.thread') || section.id !== `t${parent}` || section.parentElement !== root) return null;
    const message = post.querySelector(':scope > .postMessage');
    if (message?.id !== `m${no}`) return null;
    return { post, article, message, ref: { origin, board, thread: parent, post: no } };
  }
  function local(ref) {
    if (ref.board !== board) return null;
    const found = canonical(document.getElementById(`p${ref.post}`));
    return found && (ref.thread === null || ref.thread === found.ref.thread) ? found : null;
  }
  function source(link) {
    const owner = projection.owner(link);
    if (owner) return owner.identity === identity && !owner.closed && owner.node.contains(link)
      ? { parent: owner, canonical: owner.source.canonical, ref: owner.context } : null;
    const original = canonical(link.closest('.post'));
    return original ? { parent: null, canonical: original, ref: original.ref } : null;
  }
  function quoteContext(link) {
    const from = source(link);
    return from ? { ...from.ref, origin } : page;
  }
  function cyclic(ref, from) {
    for (let parent = from.parent; parent; parent = parent.source.parent) if (samePost(ref, parent.context)) return true;
    return samePost(ref, from.canonical.ref);
  }
  function hoverEligible(link) {
    if (!enabled() || !link?.matches?.('a.quotelink') || !root.contains(link)
      || link.closest('.post-hidden,.native-thread-hidden,[hidden]')) return false;
    const from = source(link), ref = from && quoteTarget(link.getAttribute('href'), { ...from.ref, origin });
    if (!ref || samePost(ref, from.ref) || cyclic(ref, from) || local(ref)) return false;
    const current = entries.get(link);
    if (current) return alive(current);
    const depth = (from.parent?.depth ?? 0) + 1;
    return depth <= bounds.depth && entries.size < bounds.open && pending < bounds.pending && fits(placeholder);
  }
  function companion(link) {
    const pair = companions.get(link);
    return pair && !pair.entry.closed && pair.entry.node.contains(link)
      && pair.node.parentNode === link.parentNode && link.nextSibling === pair.node
      && pair.node.getAttribute('href') === link.getAttribute('href') ? pair.node : null;
  }
  function placement(link, target) {
    const message = target && backlinkOwner?.(link);
    if (message && root.contains(message)) return { parent: message, before: message.firstChild, message };
    let after = link;
    if (link.parentElement.classList.contains('quote')) after = link.parentElement;
    else {
      const pair = companion(link) ?? externalCompanion?.(link);
      // Keep the source link and its exact owned mobile # adjacent.
      if (pair && pair.parentNode === link.parentNode && link.nextSibling === pair
        && pair.matches('a.quoteLink') && pair.getAttribute('href') === link.getAttribute('href')) after = pair;
    }
    while (after.parentElement?.nodeName === 'S') after = after.parentElement;
    return { parent: after.parentNode, before: after.nextSibling, message: null };
  }
  function fits(cost, previous = { nodes: 0, characters: 0 }) {
    return nodes - previous.nodes + cost.nodes <= bounds.nodes
      && characters - previous.characters + cost.characters <= bounds.characters;
  }
  function prepare(tree, context, ref, target, link) {
    const plan = prepareQuotePost(tree, context, ref.post);
    const copied = target ? prepareBacklinks?.(target.post, link) : null;
    const mobileLinks = mobileDevice ? plan.quotes.filter(href => quoteTarget(href, context)) : [];
    if (mobileLinks.length > bounds.quotes) throw new RangeError('inline-quotes');
    const cost = { nodes: plan.nodes + (copied?.nodes ?? 0) + mobileLinks.length * 2,
      characters: plan.characters + 64 + (copied?.characters ?? 0)
        + mobileLinks.reduce((sum, href) => sum + href.length + 64, 0) };
    if (cost.nodes > 16384 || cost.characters > 262144) throw new RangeError('inline-post-limit');
    return { plan, copied, cost, context };
  }
  function mark(entry, name, value) {
    if (!entry.attributes.has(name)) entry.attributes.set(name, { previous: entry.link.getAttribute(name), value });
    else entry.attributes.get(name).value = value;
    entry.link.setAttribute(name, value);
  }
  function unmark(entry, name) {
    const attr = entry.attributes.get(name);
    if (!attr) return;
    if (entry.link.getAttribute(name) === attr.value) {
      if (attr.previous === null) entry.link.removeAttribute(name); else entry.link.setAttribute(name, attr.previous);
    }
    entry.attributes.delete(name);
  }
  function hideOriginal(entry) {
    const article = entry.target.article;
    let state = hidden.get(article);
    if (!state) {
      state = { count: 0, display: article.style.display, attribute: article.getAttribute('data-inline-count') };
      hidden.set(article, state); article.style.display = 'none';
    }
    state.count++; article.setAttribute('data-inline-count', String(state.count)); entry.hidden = article;
  }
  function releaseOriginal(entry) {
    const article = entry.hidden, state = hidden.get(article);
    if (!state) return;
    const ownedCount = article.getAttribute('data-inline-count') === String(state.count);
    if (--state.count) { if (ownedCount) article.setAttribute('data-inline-count', String(state.count)); }
    else {
      if (article.style.display === 'none') article.style.display = state.display;
      if (ownedCount) {
        if (state.attribute === null) article.removeAttribute('data-inline-count');
        else article.setAttribute('data-inline-count', state.attribute);
      }
      hidden.delete(article);
    }
  }
  function stopPending(entry) {
    if (!entry.pending) return;
    entry.pending = false; pending--; clearTimeout(entry.deadline);
    queue = queue.filter(item => item !== entry); unmark(entry, 'data-loading');
    if (activeTask?.entry === entry) { activeTask = null; entry.controller.abort(); transport.cancel?.(); }
  }
  function close(entry) {
    if (!entry || entry.closed) return;
    entry.closed = true;
    for (const child of [...entry.children]) close(child);
    stopPending(entry); entry.controller.abort(); entry.cleanup?.(); entry.node?.remove();
    releaseOriginal(entry);
    for (const name of [...entry.attributes.keys()]) unmark(entry, name);
    if (!entry.faded) entry.link.classList.remove('linkfade');
    if (!entry.dead) entry.link.classList.remove('deadlink');
    entry.releaseAttributes();
    entries.delete(entry.link); entry.source.parent?.children.delete(entry);
    nodes -= entry.cost.nodes; characters -= entry.cost.characters;
    schedulePump();
  }
  function alive(entry) {
    if (entry.closed || !enabled() || !root.contains(entry.link) || entry.link.parentNode !== entry.anchorParent
      || entry.link.getAttribute('href') !== entry.href || !entry.link.classList.contains('quotelink')
      || entry.link.closest('.post-hidden,.native-thread-hidden,[hidden]')) return false;
    const from = source(entry.link);
    if (!from || from.parent !== entry.source.parent || from.canonical.post !== entry.source.canonical.post) return false;
    if (entry.node && (entry.node.parentNode !== entry.location.parent || !root.contains(entry.node))) return false;
    if ((entry.target && backlinkOwner?.(entry.link) || null) !== entry.location.message) return false;
    return !entry.target || local(entry.ref)?.post === entry.target.post;
  }
  function finish(entry, prepared) {
    if (!alive(entry)) { close(entry); return; }
    if (cyclic({ ...entry.ref, thread: prepared.context.thread }, entry.source)) { close(entry); return; }
    if (!fits(prepared.cost, entry.cost)) { fail(entry, 'Quote is too large to inline.'); return; }
    // Admission precedes every element allocation and resource assignment.
    const copy = prepared.plan.build(document);
    copy.classList.add('preview', 'inlined'); copy.setAttribute('data-inline-state', 'ready');
    projection.claim(copy, entry);
    if (mobileDevice) for (const link of copy.querySelectorAll('a.quotelink')) {
      if (!quoteTarget(link.getAttribute('href'), prepared.context)) continue;
      const node = document.createElement('a'); node.className = 'quoteLink';
      node.setAttribute('href', link.getAttribute('href')); node.textContent = ' #';
      link.after(node); companions.set(link, { entry, node });
    }
    entry.cleanup = prepared.copied?.mount(copy);
    nodes += prepared.cost.nodes - entry.cost.nodes; characters += prepared.cost.characters - entry.cost.characters;
    entry.cost = prepared.cost; entry.context = { ...prepared.context, post: entry.ref.post };
    if (entry.node) entry.node.replaceWith(copy); else entry.location.parent.insertBefore(copy, entry.location.before);
    entry.node = copy; entry.status = 'ready';
    stopPending(entry); mark(entry, 'aria-expanded', 'true'); entry.link.classList.add('linkfade');
    if (entry.target && entry.location.message) hideOriginal(entry);
    schedulePump();
  }
  function fail(entry, message = 'Error: Quote could not be loaded.', unavailable = false) {
    if (entry.closed) return;
    stopPending(entry);
    entry.status = unavailable ? 'unavailable' : 'error';
    entry.node.setAttribute('data-inline-state', entry.status); entry.node.textContent = message;
    if (unavailable) entry.link.classList.add('deadlink');
    schedulePump();
  }
  function schedulePump(delay = 0) {
    if (disposed || suspended || activeTask || wake !== null || !queue.length) return;
    wake = setTimeout(() => { wake = null; pump(); }, delay);
  }
  function pump() {
    if (activeTask || !enabled()) return;
    const entry = queue.find(item => item.pending && !item.closed);
    if (!entry) return;
    if (!alive(entry)) { close(entry); schedulePump(); return; }
    const task = { entry }; activeTask = task;
    void Promise.resolve().then(() => transport.load(entry.ref, { signal: entry.controller.signal })).then(result => {
      if (activeTask !== task) return;
      activeTask = null;
      if (!alive(entry)) { close(entry); return; }
      if (result.status === 'cooldown' || result.status === 'busy') {
        schedulePump(result.status === 'busy' ? 300 : Math.min(300, Math.max(1, result.retryAfter || 300))); return;
      }
      if (result.status === 'http-error' && result.httpStatus === 404) {
        // The one-post endpoint cannot distinguish a missing post from a
        // missing thread. Neither result is cached across another click.
        fail(entry, 'This post or thread is unavailable.', true); return;
      }
      const checked = checkedQuotePreview(result, { ...entry.ref, origin, mediaOrigin });
      if (checked.status !== 'ok') { fail(entry); return; }
      finish(entry, prepare(checked.snapshot.post.tree, checked.context, entry.ref, null, entry.link));
    }).catch(() => {
      if (activeTask === task) activeTask = null;
      if (!entry.closed && entry.pending) fail(entry);
    });
  }
  function click(event) {
    if (event.defaultPrevented) return 'ordinarynav';
    const link = event.target;
    if (!enabled() || event.button !== 0 || !link?.matches?.('a.quotelink') || !root.contains(link)) return 'passpreview';
    const from = source(link), ref = from && quoteTarget(link.getAttribute('href'), { ...from.ref, origin });
    if (!ref || link.closest('.post-hidden,.native-thread-hidden,[hidden]')) return 'ordinarynav';
    if (event.shiftKey) { event.preventDefault(); navigate(link.href); return 'ordinarynav'; }
    const current = entries.get(link);
    if (current && alive(current)) {
      event.preventDefault();
      // v1191 ignores repeated clicks while the request is loading. Errors and
      // completed copies are removed on the next click; another click retries.
      if (!current.pending) close(current);
      return 'inlinehandled';
    }
    if (current) close(current);
    if (samePost(ref, from.ref)) return 'ordinarynav';
    if (cyclic(ref, from)) { event.preventDefault(); return 'inlinehandled'; }
    const depth = (from.parent?.depth ?? 0) + 1;
    if (depth > bounds.depth || entries.size >= bounds.open) return 'ordinarynav';
    const target = local(ref), location = placement(link, target);
    let prepared = null;
    try {
      if (target) {
        const context = { origin, mediaOrigin, board: ref.board, thread: target.ref.thread };
        prepared = prepare(localQuoteTree(target.article, context, ref.post, projection), context, ref, target, link);
      } else if (pending >= bounds.pending) return 'ordinarynav';
    } catch { return 'ordinarynav'; }
    const cost = prepared?.cost ?? placeholder;
    if (!fits(cost)) return 'ordinarynav';
    const entry = { identity, link, href: link.getAttribute('href'), anchorParent: link.parentNode,
      source: from, ref, context: { ...ref, origin }, target, location, depth, children: new Set(),
      controller: new AbortController(), attributes: new Map(), faded: link.classList.contains('linkfade'),
      dead: link.classList.contains('deadlink'), cost, node: null, closed: false, pending: false };
    const originalClass = link.getAttribute('class');
    entry.releaseAttributes = projection.trackAttributes(link, (name, value) => {
      if (name === 'class') {
        const tokens = value.split(/\s+/).filter(token => (entry.faded || token !== 'linkfade') && (entry.dead || token !== 'deadlink'));
        const normalized = tokens.join(' ');
        return normalized === originalClass?.split(/\s+/).join(' ') ? originalClass : normalized;
      }
      const attribute = entry.attributes.get(name);
      return attribute && attribute.value === value ? attribute.previous : value;
    });
    entries.set(link, entry); from.parent?.children.add(entry); nodes += cost.nodes; characters += cost.characters;
    event.preventDefault();
    if (prepared) {
      try { finish(entry, prepared); } catch { close(entry); }
    } else {
      const node = document.createElement('div'); node.className = 'preview spinner inlined';
      node.setAttribute('role', 'status'); node.setAttribute('data-inline-state', 'loading'); node.textContent = 'Loading...';
      projection.claim(node, entry); entry.node = node; entry.status = 'loading'; entry.pending = true; pending++;
      mark(entry, 'data-loading', '1'); location.parent.insertBefore(node, location.before);
      entry.deadline = setTimeout(() => fail(entry, 'Error: Quote request timed out.'), bounds.pendingMs);
      queue.push(entry); schedulePump();
    }
    return 'inlinehandled';
  }
  function clear() {
    clearTimeout(wake); wake = null;
    for (const entry of [...entries.values()]) close(entry);
    clearTimeout(wake); wake = null; queue = []; transport.cancel?.();
  }
  function refresh() {
    if (!enabled()) { clear(); return; }
    for (const entry of [...entries.values()]) if (!alive(entry)) close(entry);
  }
  const observer = new window.MutationObserver(() => {
    if (scheduled || disposed || suspended || !entries.size) return;
    scheduled = true; queueMicrotask(() => { scheduled = false; refresh(); });
  });
  const observe = () => observer.observe(root, { childList: true, subtree: true,
    attributes: true, attributeFilter: ['href', 'class', 'id', 'hidden'] });
  const settingsChanged = () => { clear(); refresh(); };
  const storage = event => { if (event.key === null || event.key === '4chan-settings') settingsChanged(); };
  const hide = event => { suspended = true; clear(); observer.disconnect(); if (!event.persisted) disconnect(); };
  const restore = event => { if (event.persisted && !disposed) { suspended = false; observe(); refresh(); } };
  function disconnect() {
    if (disposed) return;
    disposed = true; clear(); observer.disconnect();
    document.removeEventListener('4chanSettingsSaved', settingsChanged);
    window.removeEventListener('storage', storage); window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', restore);
  }
  document.addEventListener('4chanSettingsSaved', settingsChanged);
  window.addEventListener('storage', storage); window.addEventListener('pagehide', hide); window.addEventListener('pageshow', restore);
  observe();
  return { click, refresh, clear, disconnect, companion, quoteContext, hoverEligible,
    stats: () => ({ open: entries.size, pending, nodes, characters, hidden: hidden.size }) };
}
