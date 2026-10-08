import { buildPostTree, checkPostTreeIds } from './native-post-tree.js';
import { parsePostRecipe, updaterContext } from './native-updater-snapshot.js';

export const SEARCH_LIMITS = Object.freeze({
  pageSize: 10,
  maxPages: 10,
  hashUnits: 512,
  maxHits: 20_000,
  bytes: 1_048_576,
  responseBytes: 1_048_576,
  reads: 4096,
  nodes: 50_000,
  depth: 32,
  postsPerThread: 6,
});

const exact = (value, keys) => value !== null && typeof value === 'object' && !Array.isArray(value)
  && Object.keys(value).sort().join(',') === [...keys].sort().join(',');
const postId = value => typeof value === 'string' && /^[1-9][0-9]{0,18}$/.test(value)
  && BigInt(value) <= 9223372036854775807n;

export function pageToOffset(page) {
  if (page < 1 || page > SEARCH_LIMITS.maxPages) page = 1;
  return (page - 1) * SEARCH_LIMITS.pageSize;
}

export function offsetToPage(offset) {
  const page = offset / SEARCH_LIMITS.pageSize + 1;
  if (page < 1 || page > SEARCH_LIMITS.maxPages) return 1;
  return page;
}

export function parseSearchHash(hash, boards = null) {
  if (typeof hash !== 'string' || hash === '') return { query: '', board: '', offset: 0 };
  // Source checks the complete UTF-16 hash before splitting or decoding.
  if (hash.length > SEARCH_LIMITS.hashUnits) return { query: '', board: '', offset: 0 };
  const fragments = hash.split('/').slice(1);
  let query;
  try { query = fragments[0] ? decodeURIComponent(fragments[0]) : ''; }
  catch { return null; }
  let board = fragments[1] || '';
  if (board === 'all') board = '';
  if (boards && board !== '' && !boards.has(board)) board = '';
  // Preserve source ToInt32 (including hexadecimal, fractions and wrapping).
  return { query, board, offset: pageToOffset(0 | fragments[2]) };
}

export function searchHash(query, board, offset) {
  if (query === '') return '';
  const fragments = [encodeURIComponent(query)];
  if (offset > 0) fragments.push(board || 'all', String(offsetToPage(offset)));
  else if (board) fragments.push(board);
  return `#/${fragments.join('/')}`;
}

async function boundedText(response, signal) {
  const mime = response.headers?.get('content-type')?.split(';')[0].trim().toLowerCase();
  if (mime !== 'application/json') {
    try { await response.body?.cancel(); } catch { /* best effort */ }
    throw new Error('invalid-content-type');
  }
  const declared = response.headers?.get('content-length');
  if (declared !== null && (!/^\d+$/.test(declared) || Number(declared) > SEARCH_LIMITS.responseBytes)) {
    try { await response.body?.cancel(); } catch { /* best effort */ }
    throw new Error('response-limit');
  }
  const body = response.body, reader = body?.getReader?.();
  if (!reader) throw new Error('missing-body');
  const cancel = () => { try { reader.cancel().catch(() => {}); } catch { /* best effort */ } };
  signal?.addEventListener('abort', cancel, { once: true });
  const parts = [];
  let bytes = 0, reads = 0;
  try {
    for (;;) {
      const part = await reader.read();
      if (signal?.aborted) throw new DOMException('aborted', 'AbortError');
      if (part.done) break;
      if (!(part.value instanceof Uint8Array) || ++reads > SEARCH_LIMITS.reads) throw new Error('response-limit');
      bytes += part.value.byteLength;
      if (bytes > SEARCH_LIMITS.responseBytes) throw new Error('response-limit');
      parts.push(part.value);
    }
    const joined = new Uint8Array(bytes);
    let offset = 0;
    for (const part of parts) { joined.set(part, offset); offset += part.byteLength; }
    return new TextDecoder('utf-8', { fatal: true }).decode(joined);
  } finally {
    signal?.removeEventListener('abort', cancel);
    try { await reader.cancel(); } catch { /* reader may already be closed */ }
  }
}

export function parseSearchPayload(raw, { origin, mediaOrigin = '' }) {
  if (typeof raw !== 'string' || new TextEncoder().encode(raw).length > SEARCH_LIMITS.responseBytes) throw new SyntaxError('invalid search response');
  let value;
  try { value = JSON.parse(raw); } catch { throw new SyntaxError('invalid search response'); }
  if (!exact(value, ['threads', 'offset', 'nhits'])
      || !Number.isInteger(value.offset) || value.offset < 0 || value.offset > 90 || value.offset % 10 !== 0
      || !Number.isInteger(value.nhits) || value.nhits < 0 || value.nhits > SEARCH_LIMITS.maxHits
      || !Array.isArray(value.threads) || value.threads.length > SEARCH_LIMITS.pageSize) throw new SyntaxError('invalid search response');
  const ids = new Set(), budget = { nodes: 0, chars: 0 };
  const threads = value.threads.map(thread => {
    if (!exact(thread, ['board', 'thread', 'posts']) || !/^[a-z0-9]{1,10}$/.test(thread.board)
        || !postId(thread.thread) || !Array.isArray(thread.posts) || thread.posts.length < 1
        || thread.posts.length > SEARCH_LIMITS.postsPerThread) throw new SyntaxError('invalid search thread');
    const context = updaterContext({ origin, board: thread.board, thread: thread.thread, mediaOrigin });
    let previous = 0n;
    const posts = thread.posts.map((post, index) => {
      if (!exact(post, ['no', 'html']) || !postId(post.no) || ids.has(post.no)
          || BigInt(post.no) <= previous || (index === 0) !== (post.no === thread.thread)
          || typeof post.html !== 'string') throw new SyntaxError('invalid search post');
      previous = BigInt(post.no); ids.add(post.no);
      return { no: post.no, tree: parsePostRecipe(post.html, context, post.no, budget, SEARCH_LIMITS) };
    });
    return { board: thread.board, thread: thread.thread, posts };
  });
  return { threads, offset: value.offset, nhits: value.nhits };
}

export async function requestSearch({ query, board = '', offset = 0, origin = globalThis.location?.origin,
  mediaOrigin = '', signal, fetcher = globalThis.fetch?.bind(globalThis) }) {
  if (!fetcher || typeof origin !== 'string') throw new Error('search transport unavailable');
  const params = new URLSearchParams({ q: query });
  if (board) params.set('b', board);
  if (offset) params.set('o', String(offset));
  const response = await fetcher(`/search/api?${params}`, {
    method: 'GET', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', cache: 'no-store', signal,
    headers: { accept: 'application/json' },
  });
  const text = await boundedText(response, signal);
  if (!response.ok) throw new Error('search request failed');
  return parseSearchPayload(text, { origin: `${new URL(origin).origin}/`, mediaOrigin });
}

export function mountGlobalSearch({ root = globalThis.document, history = globalThis.history, location = globalThis.location,
  scope = globalThis, fetcher = globalThis.fetch?.bind(globalThis), mobile = globalThis.matchMedia?.('(max-width: 480px)') } = {}) {
  const form = root?.getElementById('g-search-form');
  if (!form || !history || !location) return null;
  const queryField = root.getElementById('js-sf-qf'), boardField = root.getElementById('js-sf-bf');
  const button = root.getElementById('js-sf-btn'), results = root.getElementById('js-sf-results');
  const anchor = root.getElementById('delform'), contextNode = root.getElementById('search-context');
  if (!queryField || !boardField || !button || !results || !anchor || !contextNode) return null;
  const boards = new Set([...boardField.options].map(option => option.value).filter(Boolean));
  const mediaOrigin = contextNode.dataset.mediaOrigin || '';
  const pageOrigin = new URL(location.href).origin;
  const state = { query: '', board: '', offset: 0, controller: null, sequence: 0 };

  function clearPager() { root.getElementById('js-sf-pl')?.remove(); }
  function status(message, error = false) {
    results.replaceChildren();
    if (!message) return;
    const node = root.createElement('div'); node.id = 'js-sf-status'; node.className = error ? 'js-sf-err' : 'js-sf-spnr'; node.textContent = message; results.append(node);
  }
  function busy(flag) { button.disabled = flag; if (flag) status('Searching…'); }
  function updateHash() {
    const hash = searchHash(state.query, state.board, state.offset), base = location.href.replace(/#.*$/, '');
    history.replaceState(null, '', hash ? `${base}${hash}` : base);
  }
  function renderPager(total) {
    clearPager();
    const maxPage = Math.min(Math.ceil(total / SEARCH_LIMITS.pageSize), SEARCH_LIMITS.maxPages), current = offsetToPage(state.offset);
    if (maxPage < 1) return;
    const pager = root.createElement('div'); pager.id = 'js-sf-pl'; pager.className = mobile?.matches ? 'mPagelist mobile' : 'pagelist desktop';
    const add = (label, offset, className) => {
      const wrap = root.createElement('div'); wrap.className = className;
      const control = root.createElement(mobile?.matches ? 'a' : 'button');
      if (control.tagName === 'A') { control.href = '#'; control.className = 'button'; } else control.type = 'button';
      control.textContent = label; control.dataset.o = String(offset);
      control.addEventListener('click', event => { event.preventDefault(); execute(state.query, state.board, offset); });
      wrap.append(control); pager.append(wrap);
    };
    if (current > 1) add('Previous', state.offset - SEARCH_LIMITS.pageSize, 'prev');
    const pages = root.createElement('div'); pages.className = 'pages'; pages.textContent = `Page ${current} / ${maxPage}`; pager.append(pages);
    if (current < maxPage) add('Next', state.offset + SEARCH_LIMITS.pageSize, 'next');
    anchor.insertAdjacentElement('afterend', pager);
  }
  function renderThreads(data) {
    results.replaceChildren();
    const trees = data.threads.flatMap(thread => thread.posts.map(post => post.tree));
    checkPostTreeIds(trees, root);
    for (const thread of data.threads) {
      const section = root.createElement('section'); section.className = 'thread searchThread'; section.id = `t${thread.thread}`;
      section.dataset.board = thread.board; section.setAttribute('aria-label', `/${thread.board}/ thread ${thread.thread}`);
      const boardBlock = root.createElement('div'); boardBlock.className = 'boardBlock';
      const boardLink = root.createElement('a'); boardLink.href = `/${thread.board}/`; boardLink.textContent = `/${thread.board}/`; boardBlock.append(boardLink); section.append(boardBlock);
      for (const post of thread.posts) section.append(buildPostTree(post.tree, root, { board: thread.board }));
      results.append(section, root.createElement('hr'));
    }
  }
  async function execute(query, board, offset) {
    clearPager(); state.controller?.abort(); state.controller = null;
    if (query === '') { busy(false); status(''); return; }
    state.query = query; state.board = boards.has(board) ? board : '';
    // Keep the replacement backend's bounded integral offsets at this boundary.
    state.offset = Number.isInteger(offset) && offset % SEARCH_LIMITS.pageSize === 0
      ? pageToOffset(offsetToPage(offset)) : 0;
    updateHash();
    const controller = new AbortController(); state.controller = controller; const sequence = ++state.sequence; busy(true);
    try {
      const data = await requestSearch({ query: state.query, board: state.board, offset: state.offset, origin: pageOrigin, mediaOrigin, signal: controller.signal, fetcher });
      if (sequence !== state.sequence || controller.signal.aborted) return;
      busy(false);
      if (data.threads.length === 0) { status('Nothing found.', true); clearPager(); return; }
      renderThreads(data); renderPager(data.nhits);
    } catch (error) {
      if (controller.signal.aborted || sequence !== state.sequence) return;
      busy(false); clearPager(); status(error instanceof SyntaxError ? 'Something went wrong.' : 'Connection error.', true);
    } finally { if (state.controller === controller) state.controller = null; }
  }
  function fromHash(initial = false) {
    const parsed = parseSearchHash(location.hash, boards);
    if (!parsed) {
      state.controller?.abort(); state.controller = null; state.sequence++; clearPager(); busy(false); status('Something went wrong.', true); return;
    }
    state.query = parsed.query; state.board = parsed.board; state.offset = parsed.offset;
    queryField.value = state.query; boardField.value = state.board;
    if (!initial || state.query !== '') execute(state.query, state.board, state.offset);
  }
  form.addEventListener('submit', event => { event.preventDefault(); execute(queryField.value, boardField.value, 0); });
  scope.addEventListener?.('hashchange', () => fromHash(false)); fromHash(true);
  return { execute, fromHash, state, destroy() { state.controller?.abort(); state.sequence++; } };
}

if (typeof document !== 'undefined') {
  const start = () => mountGlobalSearch();
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true });
  else start();
}
