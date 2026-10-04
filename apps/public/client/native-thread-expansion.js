// This import remains external in the release asset. The parser, transport and
// post validator are the same bounded implementations used by thread updates.
import { NativeUpdaterTransport, UPDATER_LIMITS, updaterContext, validatePostTree, validateSnapshotMetadata } from '../static/native-filter.v1.js';
import { buildPostTree, checkPostTreeIds } from './native-post-tree.js';
import { postId } from '../static/thread-watcher-core.v1.js';

export const EXPANSION_LIMITS = Object.freeze({ threads: 100, posts: 1000, nodes: 100000, bytes: 4194304, applyMs: 10000 });

export function planThreadExpansion(snapshot, context, originalIds) {
  validateSnapshotMetadata(snapshot, context);
  if (snapshot.tail_id !== null || !Array.isArray(originalIds) || !originalIds.length || originalIds.length > 6
    || originalIds[0] !== context.thread || originalIds.some((id, index) => postId(id) !== id
      || (index > 0 && BigInt(id) <= BigInt(originalIds[index - 1])))) throw new TypeError('index-context');
  let previous = 0n;
  const budget = { nodes: 0 }, additions = [];
  for (const post of snapshot.posts) {
    if (postId(post.no) !== post.no || BigInt(post.no) <= previous || typeof post.file_deleted !== 'boolean') throw new TypeError('post-order');
    previous = BigInt(post.no);
    validatePostTree(post.tree, context, post.no, budget);
    if (post.no !== context.thread && (!originalIds[1] || BigInt(post.no) < BigInt(originalIds[1]))) additions.push(post);
  }
  if (snapshot.posts[0].no !== context.thread) throw new TypeError('missing-op');
  // Account for the retained recipe's text and attributes before constructing
  // resource-bearing elements. All entries on this page share these limits.
  const encoder = new TextEncoder();
  const cost = { posts: additions.length, nodes: 0, bytes: 0 };
  function charge(tree) {
    cost.nodes++;
    if (typeof tree === 'string') cost.bytes += encoder.encode(tree).length;
    else {
      cost.bytes += tree.tag.length + 5;
      for (const [name, value] of Object.entries(tree.attrs)) cost.bytes += name.length + encoder.encode(value).length + 4;
      for (const child of tree.children) charge(child);
    }
    if (cost.nodes > EXPANSION_LIMITS.nodes || cost.bytes > EXPANSION_LIMITS.bytes) throw new RangeError('expansion-budget');
  }
  for (const post of additions) charge(post.tree);
  return { additions, cost };
}

export function mountNativeThreadExpansion({ root, board, thread, mediaOrigin = '', settings, applied, projection,
  decorateButton, origin = globalThis.location?.origin,
  createTransport = context => new NativeUpdaterTransport(context), limits = {} }) {
  const document = root?.ownerDocument, window = document?.defaultView;
  if (!root || !window || thread || !/^[a-z0-9]{1,10}$/.test(board)) return null;
  const bound = { ...EXPANSION_LIMITS };
  for (const [name, value] of Object.entries(limits)) {
    if (!(name in bound) || !Number.isInteger(value) || value < 1 || value > bound[name]) throw new RangeError('expansion-limit');
    bound[name] = value;
  }
  const entries = new Map(), used = { posts: 0, nodes: 0, bytes: 0 };
  let active = null, suspended = false, destroyed = false, lastStart = -Infinity;
  const disabled = () => destroyed || suspended || settings().disableAll === true || settings().threadExpansion === false;
  const available = () => !disabled() && !document.hidden && window.navigator.onLine !== false;
  function buttonState(entry) {
    const busy = active?.entry === entry;
    const label = `${busy ? 'Cancel expansion of' : entry.expanded ? 'Collapse' : 'Expand'} thread ${entry.id}`;
    entry.button.setAttribute('aria-label', label); entry.button.title = label;
    entry.button.setAttribute('aria-expanded', String(entry.expanded));
    entry.button.setAttribute('aria-busy', String(busy));
    const image = busy ? 'post_expand_rotate' : entry.expanded ? 'post_expand_minus' : 'post_expand_plus';
    if (decorateButton) decorateButton(entry.button, image, label);
    else entry.button.textContent = busy ? '[×]' : entry.expanded ? '[−]' : '[+]';
  }
  function discard(entry) {
    for (const { element, release } of entry.added) { element.remove(); release?.(); }
    entry.added = [];
    for (const key of Object.keys(used)) used[key] -= entry.cost[key];
    entry.cost = { posts: 0, nodes: 0, bytes: 0 }; entry.loaded = false; entry.expanded = false;
    entry.original.hidden = false;
    entry.section.classList.remove('tExpanded', 'tCollapsed');
  }
  function retire(entry) {
    if (active?.entry === entry) cancel();
    discard(entry);
    entry.button.remove(); entry.state.remove();
    entry.original.replaceWith(...entry.original.childNodes);
    entries.delete(entry.section);
  }
  function cancel(message = '') {
    const request = active;
    if (!request) return;
    active = null; request.controller.abort(); request.transport.cancel();
    discard(request.entry);
    request.entry.state.textContent = message; buttonState(request.entry);
  }
  function show(entry, expanded) {
    entry.expanded = expanded;
    entry.section.classList.toggle('tExpanded', expanded);
    entry.section.classList.toggle('tCollapsed', !expanded);
    entry.original.hidden = expanded;
    entry.state.textContent = expanded ? `Showing ${entry.added.length} earlier replies. ` : '';
    buttonState(entry);
  }
  async function toggle(entry) {
    if (disabled() || !entry.section.isConnected) return;
    if (active?.entry === entry) { cancel('Expansion cancelled. '); return; }
    if (entry.loaded) { show(entry, !entry.expanded); return; }
    if (!available()) { entry.state.textContent = 'Expansion is unavailable while the page is hidden or offline. '; return; }
    if (active) { entry.state.textContent = 'Another thread is loading. Try again after it finishes. '; return; }
    if (window.performance.now() - lastStart < UPDATER_LIMITS.intervalMs) { entry.state.textContent = 'Please wait before expanding again. '; return; }
    lastStart = window.performance.now();
    const context = updaterContext({ origin, board, thread: entry.id, mediaOrigin });
    const controller = new AbortController(), transport = createTransport(context);
    const request = { entry, controller, transport }; active = request;
    entry.state.textContent = 'Loading omitted replies... '; buttonState(entry);
    const stillCurrent = () => active === request && !controller.signal.aborted && available()
      && entry.section.isConnected && entries.get(entry.section) === entry;
    try {
      const result = await transport.refresh({ signal: controller.signal });
      if (!stillCurrent()) return;
      if (result.status !== 'ok') {
        entry.state.textContent = result.status === 'http-error' && result.httpStatus === 404
          ? 'Thread no longer exists. ' : 'Could not load replies. Open the thread or retry. ';
        return;
      }
      const originals = [...entry.section.querySelectorAll(':scope > .postContainer')];
      const ids = originals.map(element => element.id.slice(2));
      const { additions, cost } = planThreadExpansion(result.snapshot, context, ids);
      if (Object.keys(used).some(key => cost[key] > bound[key] - used[key])) {
        entry.state.textContent = 'Page expansion limit reached. Open the thread to read its replies. '; return;
      }
      checkPostTreeIds(additions.map(post => post.tree), document);
      const fragment = document.createDocumentFragment();
      for (const post of additions) {
        const element = buildPostTree(post.tree, document, { board });
        element.classList.add('rExpanded');
        const release = projection?.trackAttributes(element, (name, value) => name === 'class'
          ? value.split(/\s+/).filter(part => part !== 'rExpanded').join(' ') : value);
        entry.added.push({ element, release }); fragment.append(element);
      }
      entry.cost = cost;
      for (const key of Object.keys(used)) used[key] += cost[key];
      entry.section.insertBefore(fragment, originals[1] ?? entry.summary);
      entry.loaded = true; show(entry, true);
      // A feature callback can include worker-backed filters. A stalled or
      // cancelled pass cannot retain partially integrated expansion state.
      let timer, abort;
      try {
        const deadline = new Promise((_, reject) => { timer = window.setTimeout(() => reject(new Error('apply-timeout')), bound.applyMs); });
        const cancelled = new Promise((_, reject) => {
          abort = () => reject(new Error('cancelled'));
          controller.signal.addEventListener('abort', abort, { once: true });
        });
        await Promise.race([Promise.resolve(applied?.(result.snapshot, controller.signal)), deadline, cancelled]);
      } finally { window.clearTimeout(timer); controller.signal.removeEventListener('abort', abort); }
      if (!stillCurrent()) { if (entries.get(entry.section) === entry) discard(entry); return; }
      document.dispatchEvent(new window.CustomEvent('4chanThreadExpanded', { detail: { thread: entry.id, count: additions.length } }));
    } catch {
      if (entries.get(entry.section) === entry && active === request) {
        discard(entry); entry.original.hidden = false;
        if (active === request) entry.state.textContent = 'Could not apply the expansion. Open the thread or retry. ';
      }
    } finally {
      controller.abort(); transport.cancel();
      if (active === request) active = null;
      if (entries.get(entry.section) === entry) buttonState(entry);
    }
  }
  function refresh() {
    for (const entry of entries.values()) if (disabled() || !root.contains(entry.section)) retire(entry);
    if (disabled()) return;
    for (const section of root.querySelectorAll(':scope > .thread')) {
      if (entries.has(section)) { buttonState(entries.get(section)); continue; }
      if (entries.size >= bound.threads) continue;
      const id = postId(section.id.slice(1)), summary = section.querySelector(':scope > .omitted');
      if (!id || !summary || summary.childNodes.length > 8) continue;
      const link = summary.querySelector('a');
      if (link?.getAttribute('href') !== `/${board}/thread/${id}`) continue;
      const original = document.createElement('span'), state = document.createElement('span');
      original.className = 'nativeExpansionOriginal'; state.className = 'nativeExpansionStatus'; state.setAttribute('role', 'status');
      original.append(...summary.childNodes);
      const button = document.createElement('button'); button.type = 'button'; button.className = 'nativeThreadExpand watcherIcon';
      const entry = { id, section, summary, original, state, button, added: [], loaded: false, expanded: false,
        cost: { posts: 0, nodes: 0, bytes: 0 } };
      button.addEventListener('click', () => { void toggle(entry); });
      entries.set(section, entry); summary.append(button, state, original); buttonState(entry);
    }
  }
  const visibility = () => { if (!available()) cancel('Expansion paused. '); };
  const storage = event => { if (event.key === null || event.key === '4chan-settings') refresh(); };
  const hide = () => { suspended = true; cancel(); refresh(); };
  const showPage = () => { suspended = false; refresh(); };
  document.addEventListener('visibilitychange', visibility);
  document.addEventListener('4chanSettingsSaved', refresh);
  window.addEventListener('storage', storage); window.addEventListener('offline', visibility);
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', showPage);
  refresh();
  return { refresh, stats: () => ({ ...used, threads: entries.size, busy: active !== null }), destroy() {
    if (destroyed) return;
    destroyed = true; cancel(); refresh();
    document.removeEventListener('visibilitychange', visibility);
    document.removeEventListener('4chanSettingsSaved', refresh);
    window.removeEventListener('storage', storage); window.removeEventListener('offline', visibility);
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', showPage);
  } };
}
