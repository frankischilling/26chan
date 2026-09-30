import { UPDATER_LIMITS, boardPageContext, updaterContext, validateBoardPageSnapshot, validatePostTree } from '../static/native-filter.v1.js';
import { postId } from '../static/thread-watcher-core.v1.js';
import { buildPostTree, checkPostTreeIds } from './native-post-tree.js';

export const DEPAGER_PAGE_LIMITS = Object.freeze({
  threads: 20,
  posts: 80,
  nodes: 100000,
  bytes: 4 * 1024 * 1024,
});

export const DEPAGER_LIMITS = Object.freeze({
  pages: 20,
  threads: 200,
  posts: 1000,
  nodes: 200000,
  bytes: 16 * 1024 * 1024,
  requestMs: 10000,
  applyMs: 10000,
  threshold: 350,
});

const PAGE_MAX = 999;

function require(value, message = 'invalid-depager-snapshot') {
  if (!value) throw new TypeError(message);
}

function normalizeContext({ origin, board, mediaOrigin = '' }) {
  const context = boardPageContext({ origin, board, mediaOrigin, page: 0 });
  return { origin: context.origin, board: context.board, mediaOrigin: context.mediaOrigin };
}

function validPage(value) {
  return Number.isInteger(value) && value >= 0 && value <= PAGE_MAX;
}

function treeCost(tree, encoder, cost) {
  cost.nodes++;
  if (typeof tree === 'string') {
    cost.bytes += encoder.encode(tree).length;
  } else {
    cost.bytes += encoder.encode(tree.tag).length + 5;
    for (const [name, value] of Object.entries(tree.attrs)) {
      cost.bytes += encoder.encode(name).length + encoder.encode(value).length + 4;
    }
    for (const child of tree.children) treeCost(child, encoder, cost);
  }
}

export function validateDepagerSnapshot(snapshot, input) {
  const context = boardPageContext(input);
  validateBoardPageSnapshot(snapshot, context);
  const serialized = { nodes: 0, bytes: 0 };
  const encoder = new TextEncoder();
  let postCount = 0;
  for (const thread of snapshot.threads) {
    postCount += thread.posts.length;
    require(postCount <= DEPAGER_PAGE_LIMITS.posts);
    for (const post of thread.posts) {
      treeCost(post.tree, encoder, serialized);
      require(serialized.nodes <= DEPAGER_PAGE_LIMITS.nodes && serialized.bytes <= DEPAGER_PAGE_LIMITS.bytes);
    }
  }
  return { context, cost: { posts: postCount, nodes: serialized.nodes, bytes: serialized.bytes } };
}

export function planDepagerPage(snapshot, input, root, limits = DEPAGER_LIMITS) {
  const document = root?.ownerDocument;
  require(document && root.matches?.('.board'), 'invalid-depager-root');
  const { context } = validateDepagerSnapshot(snapshot, input);
  const encoder = new TextEncoder();
  const cost = { threads: 0, posts: 0, nodes: 0, bytes: 0 };
  const additions = [];
  for (const thread of snapshot.threads) {
    const live = document.getElementById(`t${thread.thread}`);
    if (live) {
      require(live.matches('.thread') && root.contains(live), 'duplicate-dom-id');
      continue;
    }
    const threadCost = { nodes: 0, bytes: 0 };
    for (const post of thread.posts) treeCost(post.tree, encoder, threadCost);
    cost.threads++;
    cost.posts += thread.posts.length;
    cost.nodes += threadCost.nodes;
    cost.bytes += threadCost.bytes;
    additions.push(thread);
  }
  require(cost.threads <= limits.threads && cost.posts <= limits.posts
    && cost.nodes <= limits.nodes && cost.bytes <= limits.bytes, 'depager-budget');
  checkPostTreeIds(additions.flatMap(thread => thread.posts.map(post => post.tree)), document);
  return { context, additions, cost };
}

export function defaultDepagerPageMarker(page, document) {
  if (!validPage(page) || !document?.createElement) return null;
  const marker = document.createElement('span');
  marker.className = 'depageNumber'; marker.dataset.nativeDepager = 'true';
  marker.textContent = `Page ${page + 1}`;
  return marker;
}

function makeThread(thread, context, document) {
  const section = document.createElement('section');
  section.className = 'thread'; section.id = `t${thread.thread}`;
  section.dataset.sticky = String(thread.sticky);
  section.dataset.closed = String(thread.closed);
  section.dataset.archived = 'false';
  section.dataset.tailSize = '0';
  section.setAttribute('aria-label', `Thread ${thread.thread}`);
  const threadContext = updaterContext({ ...context, thread: thread.thread });
  for (const post of thread.posts) {
    validatePostTree(post.tree, threadContext, post.no);
    section.append(buildPostTree(post.tree, document));
  }
  if (thread.omitted > 0) {
    const summary = document.createElement('p'); summary.className = 'omitted';
    summary.append(`${thread.omitted} posts omitted. `);
    const link = document.createElement('a');
    link.href = `/${context.board}/thread/${thread.thread}`;
    link.textContent = 'View thread'; summary.append(link); section.append(summary);
  }
  return section;
}

function deadline(window, promise, milliseconds, signal, label) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const finish = (callback, value) => {
      if (settled) return;
      settled = true; window.clearTimeout(timer); signal?.removeEventListener('abort', aborted); callback(value);
    };
    const aborted = () => finish(reject, new Error('cancelled'));
    const timer = window.setTimeout(() => finish(reject, new Error(label)), milliseconds);
    signal?.addEventListener('abort', aborted, { once: true });
    Promise.resolve(promise).then(value => finish(resolve, value), error => finish(reject, error));
  });
}

export function mountNativeDepager({ root, board, page = 0, nextPage: initialNextPage = undefined,
  mediaOrigin = '', settings = () => ({}), createTransport, applied, stateChanged,
  pageMarker = defaultDepagerPageMarker, origin = globalThis.location?.origin, limits = {} }) {
  const document = root?.ownerDocument, window = document?.defaultView;
  if (!root || !window || !root.matches('.board') || typeof createTransport !== 'function' || !validPage(page)) return null;
  let context;
  try { context = normalizeContext({ origin, board, mediaOrigin }); } catch { return null; }
  if (initialNextPage !== undefined
    && initialNextPage !== null
    && (!validPage(initialNextPage) || initialNextPage !== page + 1)) return null;
  const bound = { ...DEPAGER_LIMITS };
  for (const [name, value] of Object.entries(limits)) {
    if (!(name in bound) || !Number.isInteger(value) || value < 1 || value > DEPAGER_LIMITS[name]) throw new RangeError('depager-limit');
    bound[name] = value;
  }
  const records = [], used = { pages: 0, threads: 0, posts: 0, nodes: 0, bytes: 0 };
  const startingNextPage = initialNextPage === undefined ? (page < PAGE_MAX ? page + 1 : null) : initialNextPage;
  let nextPage = startingNextPage;
  let active = null, suspended = false, destroyed = false, failed = false, autoBound = false;
  let state = nextPage === null ? 'complete' : 'idle';

  const readSettings = () => { try { return settings?.() ?? {}; } catch { return { disableAll: true }; } };
  const disabled = () => destroyed || readSettings().disableAll === true;
  const available = () => !disabled() && !suspended && root.isConnected && !document.hidden && window.navigator.onLine !== false;
  const auto = () => available() && readSettings().alwaysDepage === true;
  function emit(next = state) {
    state = next;
    try { stateChanged?.({ state, auto: auto(), complete: nextPage === null, nextPage }); } catch { /* UI callback is advisory. */ }
  }
  function unbindAuto() {
    if (!autoBound) return;
    autoBound = false; window.removeEventListener('scroll', onScroll); window.removeEventListener('resize', onScroll);
  }
  function bindAuto() {
    if (autoBound || !auto()) return;
    autoBound = true; window.addEventListener('scroll', onScroll, { passive: true }); window.addEventListener('resize', onScroll);
  }
  function removeRecord(record) { for (const node of record.nodes) node.remove(); }
  function resetPages() {
    for (let index = records.length - 1; index >= 0; index--) removeRecord(records[index]);
    records.length = 0;
    for (const key of Object.keys(used)) used[key] = 0;
    nextPage = startingNextPage; failed = false;
  }
  function cancel() {
    const request = active;
    if (!request) return;
    active = null; request.controller.abort();
    try { request.transport.cancel?.(); } catch { /* Cleanup still completes. */ }
    if (request.record) removeRecord(request.record);
    emit(available() ? (nextPage === null ? 'complete' : 'idle') : 'paused');
  }
  function ensureBudget(cost) {
    return used.pages < bound.pages && cost.threads <= bound.threads - used.threads
      && cost.posts <= bound.posts - used.posts && cost.nodes <= bound.nodes - used.nodes
      && cost.bytes <= bound.bytes - used.bytes;
  }
  function commitRecord(record, cost) {
    records.push(record); used.pages++;
    for (const key of ['threads', 'posts', 'nodes', 'bytes']) used[key] += cost[key];
  }
  function onScroll() {
    if (!auto() || active || failed || nextPage === null) return;
    if (document.documentElement.scrollHeight <= Math.ceil(window.innerHeight + window.scrollY) + bound.threshold) {
      void loadMore(false);
    }
  }
  function refresh() {
    if (destroyed) return;
    if (disabled()) {
      cancel(); unbindAuto(); resetPages(); emit('disabled'); return;
    }
    if (!available()) { cancel(); unbindAuto(); emit('paused'); return; }
    if (auto()) bindAuto(); else unbindAuto();
    emit(nextPage === null ? 'complete' : failed ? 'error' : active ? state : 'idle');
    if (auto()) queueMicrotask(onScroll);
  }

  async function loadMore(manual = true) {
    if (destroyed || disabled()) return { status: 'disabled' };
    if (!available()) { emit('paused'); return { status: 'unavailable' }; }
    if (active) return { status: 'busy' };
    if (nextPage === null) { emit('complete'); return { status: 'complete' }; }
    if (used.pages >= bound.pages) { failed = true; emit('limit'); return { status: 'limit' }; }
    if (manual) failed = false;
    else if (failed) return { status: 'error' };

    const requestedPage = nextPage;
    const controller = new AbortController();
    let transport;
    try {
      transport = createTransport({ ...context, limits: {
        bytes: DEPAGER_PAGE_LIMITS.bytes, requestMs: bound.requestMs, parseMs: UPDATER_LIMITS.parseMs,
      } });
      require(transport && typeof transport.refresh === 'function', 'invalid-depager-transport');
    } catch {
      failed = true; emit('error'); return { status: 'unavailable' };
    }
    const request = { controller, transport, record: null }; active = request; emit('loading');
    const current = () => active === request && !controller.signal.aborted && available();
    const cancelled = () => {
      if (active === request && !controller.signal.aborted) emit(disabled() ? 'disabled' : 'paused');
      return { status: 'cancelled' };
    };
    try {
      const result = await deadline(window,
        transport.refresh({ page: requestedPage, signal: controller.signal }), bound.requestMs, controller.signal, 'depager-timeout');
      if (!current()) return cancelled();
      if (result?.status !== 'ok') {
        failed = true; emit('error'); return result && typeof result.status === 'string' ? result : { status: 'invalid-response' };
      }
      const snapshot = result.snapshot;
      const { additions, cost } = planDepagerPage(snapshot, { ...context, page: requestedPage }, root, bound);
      if (!ensureBudget(cost)) { failed = true; emit('limit'); return { status: 'limit' }; }

      const fragment = document.createDocumentFragment(), nodes = [], sections = [];
      if (pageMarker) {
        const marker = pageMarker(snapshot.page, document);
        require(!marker || (marker.ownerDocument === document && !marker.isConnected), 'invalid-page-marker');
        if (marker) { fragment.append(marker); nodes.push(marker); }
      }
      for (const thread of additions) {
        const section = makeThread(thread, context, document);
        fragment.append(section); nodes.push(section); sections.push(section);
      }
      const record = { page: snapshot.page, nodes, sections }; request.record = record;
      const x = window.scrollX, y = window.scrollY;
      root.append(fragment); window.scrollTo(x, y); emit('applying');
      await deadline(window,
        Promise.resolve(applied?.({ page: snapshot.page, threads: snapshot.threads, added: sections }, controller.signal)),
        bound.applyMs, controller.signal, 'depager-apply-timeout');
      if (!current()) { removeRecord(record); request.record = null; return cancelled(); }
      require(record.nodes.every(node => node.parentNode === root && node.isConnected), 'depager-apply-integrity');
      request.record = null; commitRecord(record, cost);
      nextPage = snapshot.next_page; failed = false;
      document.dispatchEvent(new window.CustomEvent('4chanPageDepaged', {
        detail: { page: snapshot.page, added: sections.length },
      }));
      emit(nextPage === null ? 'complete' : 'idle');
      if (auto()) queueMicrotask(onScroll);
      return { status: 'ok', page: snapshot.page, added: sections.length };
    } catch (error) {
      if (request.record) { removeRecord(request.record); request.record = null; }
      if (controller.signal.aborted || !available() || active !== request) return cancelled();
      failed = true; emit(error?.message === 'depager-budget' ? 'limit' : 'error');
      return { status: error?.message === 'depager-budget' ? 'limit' : 'invalid-snapshot' };
    } finally {
      controller.abort();
      try { transport.cancel?.(); } catch { /* Cleanup failure must not retain the slot. */ }
      if (active === request) active = null;
      if (!destroyed && available() && state === 'applying') emit(nextPage === null ? 'complete' : failed ? 'error' : 'idle');
    }
  }

  const visibility = () => { if (!available()) cancel(); else refresh(); };
  const storage = event => { if (event.key === null || event.key === '4chan-settings') refresh(); };
  const hide = () => { suspended = true; cancel(); unbindAuto(); emit('paused'); };
  const show = () => { suspended = false; refresh(); };
  document.addEventListener('visibilitychange', visibility);
  document.addEventListener('4chanSettingsSaved', refresh);
  window.addEventListener('storage', storage); window.addEventListener('online', visibility); window.addEventListener('offline', visibility);
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
  refresh();
  return {
    loadMore: () => loadMore(true), refresh, cancel,
    stats: () => ({ ...used, state, nextPage, busy: active !== null, auto: auto() }),
    destroy() {
      if (destroyed) return;
      destroyed = true; cancel(); unbindAuto(); resetPages();
      document.removeEventListener('visibilitychange', visibility);
      document.removeEventListener('4chanSettingsSaved', refresh);
      window.removeEventListener('storage', storage); window.removeEventListener('online', visibility); window.removeEventListener('offline', visibility);
      window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
      emit('disabled');
    },
  };
}
