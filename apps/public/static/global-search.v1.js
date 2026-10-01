export const SEARCH_LIMITS = Object.freeze({ pageSize: 10, maxPages: 10, hashUnits: 512, maxHits: 20000 });

export function pageToOffset(page) {
  const value = Number.parseInt(page, 10);
  if (!Number.isInteger(value) || value < 1 || value > SEARCH_LIMITS.maxPages) return 0;
  return (value - 1) * SEARCH_LIMITS.pageSize;
}

export function offsetToPage(offset) {
  const value = Number(offset);
  const page = value / SEARCH_LIMITS.pageSize + 1;
  if (!Number.isInteger(page) || page < 1 || page > SEARCH_LIMITS.maxPages) return 1;
  return page;
}

export function parseSearchHash(hash, boards = null) {
  if (typeof hash !== 'string' || hash === '') {
    return { query: '', board: '', offset: 0 };
  }
  const fragment = hash.startsWith('#/') ? hash.slice(2) : hash.slice(1);
  if (fragment.length > SEARCH_LIMITS.hashUnits) return { query: '', board: '', offset: 0 };
  const fragments = fragment.split('/');
  let query;
  try { query = fragments[0] ? decodeURIComponent(fragments[0]) : ''; }
  catch { return null; }
  let board = fragments[1] || '';
  if (board === 'all') board = '';
  if (boards && board !== '' && !boards.has(board)) board = '';
  return { query, board, offset: pageToOffset(fragments[2]) };
}

export function searchHash(query, board, offset) {
  if (query === '') return '';
  const fragments = [encodeURIComponent(query)];
  if (offset > 0) {
    fragments.push(board || 'all', String(offsetToPage(offset)));
  } else if (board) {
    fragments.push(board);
  }
  return `#/${fragments.join('/')}`;
}

export function parseSearchPayload(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
      || !Number.isInteger(value.offset) || value.offset < 0 || value.offset > 90 || value.offset % 10 !== 0
      || !Number.isInteger(value.nhits) || value.nhits < 0 || value.nhits > SEARCH_LIMITS.maxHits
      || !Array.isArray(value.threads) || value.threads.length > SEARCH_LIMITS.pageSize) throw new SyntaxError('invalid search response');
  for (const thread of value.threads) {
    if (!thread || typeof thread !== 'object' || Array.isArray(thread)
        || !/^[a-z0-9]{1,10}$/.test(thread.board) || !Array.isArray(thread.posts)
        || thread.posts.length < 1 || thread.posts.length > 6 || typeof thread.html !== 'string'
        || thread.html.length > 200000) throw new SyntaxError('invalid search thread');
  }
  return value;
}

export async function requestSearch({ query, board = '', offset = 0, signal, fetcher = globalThis.fetch?.bind(globalThis) }) {
  if (!fetcher) throw new Error('search transport unavailable');
  const params = new URLSearchParams({ q: query });
  if (board) params.set('b', board);
  if (offset) params.set('o', String(offset));
  const response = await fetcher(`/search/api?${params}`, {
    method: 'GET', credentials: 'same-origin', mode: 'same-origin', redirect: 'error', signal,
    headers: { accept: 'application/json' },
  });
  if (!response.ok || !response.headers.get('content-type')?.toLowerCase().startsWith('application/json')) throw new Error('search request failed');
  return parseSearchPayload(await response.json());
}

export function mountGlobalSearch({ root = globalThis.document, history = globalThis.history, location = globalThis.location,
  scope = globalThis, fetcher = globalThis.fetch?.bind(globalThis), mobile = globalThis.matchMedia?.('(max-width: 480px)') } = {}) {
  const form = root.getElementById('g-search-form');
  if (!form) return null;
  const queryField = root.getElementById('js-sf-qf');
  const boardField = root.getElementById('js-sf-bf');
  const button = root.getElementById('js-sf-btn');
  const results = root.getElementById('js-sf-results');
  const anchor = root.getElementById('delform');
  if (!queryField || !boardField || !button || !results || !anchor) return null;
  const boards = new Set([...boardField.options].map(option => option.value).filter(Boolean));
  const state = { query: '', board: '', offset: 0, controller: null, sequence: 0 };

  function clearPager() { root.getElementById('js-sf-pl')?.remove(); }
  function status(message, error = false) {
    results.replaceChildren();
    if (!message) return;
    const node = root.createElement('div'); node.id = 'js-sf-status'; node.className = error ? 'js-sf-err' : 'js-sf-spnr'; node.textContent = message; results.append(node);
  }
  function busy(flag) { button.disabled = flag; if (flag) status('Searching…'); }
  function updateHash() {
    const hash = searchHash(state.query, state.board, state.offset);
    const base = location.href.replace(/#.*$/, '');
    history.replaceState(null, '', hash ? `${base}${hash}` : base);
  }
  function renderPager(total) {
    clearPager();
    const maxPage = Math.min(Math.ceil(total / SEARCH_LIMITS.pageSize), SEARCH_LIMITS.maxPages);
    const current = offsetToPage(state.offset);
    if (maxPage < 1) return;
    const pager = root.createElement('div'); pager.id = 'js-sf-pl'; pager.className = mobile?.matches ? 'mPagelist mobile' : 'pagelist desktop';
    const add = (label, offset, className) => {
      const wrap = root.createElement('div'); wrap.className = className;
      const control = root.createElement(mobile?.matches ? 'a' : 'button');
      if (control.tagName === 'A') { control.href = '#'; control.className = 'button'; }
      else control.type = 'button';
      control.textContent = label; control.dataset.o = String(offset);
      control.addEventListener('click', event => { event.preventDefault(); execute(state.query, state.board, offset); });
      wrap.append(control); pager.append(wrap);
    };
    if (current > 1) add('Previous', state.offset - SEARCH_LIMITS.pageSize, 'prev');
    const pages = root.createElement('div'); pages.className = 'pages'; pages.textContent = `Page ${current} / ${maxPage}`; pager.append(pages);
    if (current < maxPage) add('Next', state.offset + SEARCH_LIMITS.pageSize, 'next');
    anchor.insertAdjacentElement('afterend', pager);
  }
  async function execute(query, board, offset) {
    clearPager();
    state.controller?.abort(); state.controller = null;
    if (query === '') { busy(false); status(''); return; }
    state.query = query; state.board = boards.has(board) ? board : ''; state.offset = pageToOffset(offsetToPage(offset)); updateHash();
    const controller = new AbortController(); state.controller = controller; const sequence = ++state.sequence; busy(true);
    try {
      const data = await requestSearch({ query: state.query, board: state.board, offset: state.offset, signal: controller.signal, fetcher });
      if (sequence !== state.sequence || controller.signal.aborted) return;
      busy(false); results.replaceChildren();
      if (data.threads.length === 0) { status('Nothing found.', true); return; }
      for (const thread of data.threads) results.insertAdjacentHTML('beforeend', thread.html);
      renderPager(data.nhits);
    } catch (error) {
      if (controller.signal.aborted || sequence !== state.sequence) return;
      busy(false); status(error instanceof SyntaxError ? 'Something went wrong.' : 'Connection error.', true);
    } finally {
      if (state.controller === controller) state.controller = null;
    }
  }
  function fromHash(initial = false) {
    const parsed = parseSearchHash(location.hash, boards);
    if (!parsed) { status('Something went wrong.', true); return; }
    state.query = parsed.query; state.board = parsed.board; state.offset = parsed.offset;
    queryField.value = state.query; boardField.value = state.board;
    if (!initial || state.query !== '') execute(state.query, state.board, state.offset);
  }
  form.addEventListener('submit', event => { event.preventDefault(); execute(queryField.value, boardField.value, 0); });
  scope.addEventListener?.('hashchange', () => fromHash(false));
  fromHash(true);
  return { execute, fromHash, state, destroy() { state.controller?.abort(); state.sequence++; } };
}

if (typeof document !== 'undefined') {
  const start = () => mountGlobalSearch();
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true });
  else start();
}
