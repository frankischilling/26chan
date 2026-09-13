import { FILTER_LIMITS } from './native-filter-limits.js';
import { nativeCommentText, nativeWatchLabel } from './native-filter-html.js';
export { FILTER_LIMITS };
export { NativeCatalogTransport, catalogApiUrl } from './native-catalog-transport.js';
export { BLACKLIST_LIMITS, readBlacklist, writeBlacklist, collectAutoWatches, planAutoWatches } from './native-auto-watch.js';
export { readFilterRules } from './native-filter-rules.js';
export { mountNativeFilters } from './native-page-filters.js';
export { mountNativeReplyHiding } from './native-reply-hiding.js';
export { mountNativeThreadHiding } from './native-thread-hiding.js';
export { mountNativeKeybinds } from './native-keybinds.js';

// Raw HTML and user patterns are evaluated only in a fresh worker, never in the client path.

const types = new Set([0, 1, 2, 4, 5, 6]);
const fields = ['trip', 'name', 'comment', 'com', 'id', 'sub', 'filename'];
const literalTypes = new Set([0, 1, 4]);
const escapedCharacters = new Set('/.*+?()[]{}\\^$');
const maximumId = '9223372036854775807';

function canonicalId(value) {
  return typeof value === 'string' && value.length <= 19 && /^[1-9][0-9]*$/.test(value)
    && (value.length < 19 || value <= maximumId);
}
function record(value) { return value !== null && typeof value === 'object' && !Array.isArray(value); }
function boardTokens(text) {
  const tokens = [];
  for (const token of text.split(/[^a-z0-9]+/i)) {
    // The native loop stops at an empty first token instead of trimming separators.
    if (!token) break;
    if (token.length > FILTER_LIMITS.boardToken) throw new Error('board-limit');
    if (!tokens.includes(token)) tokens.push(token);
    if (tokens.length > FILTER_LIMITS.boards) throw new Error('board-limit');
  }
  return tokens;
}
function filterRows(value) {
  if (!Array.isArray(value) || value.length > FILTER_LIMITS.filters) throw new Error('filter-limit');
  return value.map(row => {
    if (!record(row) || !types.has(row.type) || typeof row.pattern !== 'string'
      || row.pattern.length > FILTER_LIMITS.patterns
      || (row.active !== undefined && typeof row.active !== 'boolean')
      || (row.auto !== undefined && typeof row.auto !== 'boolean')) throw new Error('invalid-filter');
    const boards = row.boards === undefined || row.boards === null || row.boards === false ? '' : row.boards;
    if (typeof boards !== 'string' || boards.length > FILTER_LIMITS.boardText) throw new Error('invalid-boards');
    boardTokens(boards);
    return { type: row.type, pattern: row.pattern, boards, active: row.active === true, auto: row.auto === true };
  });
}

export function readNativeFilters(raw) {
  if (raw === null) return { status: 'ok', filters: [] };
  if (typeof raw !== 'string' || raw.length > FILTER_LIMITS.settings) return { status: 'invalid-settings' };
  try { return { status: 'ok', filters: filterRows(JSON.parse(raw)) }; }
  catch { return { status: 'invalid-settings' }; }
}

export function autoWatchBoards(filters) {
  try {
    const boards = [];
    for (const filter of filterRows(filters)) {
      if (!filter.active || !filter.auto || filter.pattern === '') continue;
      for (const board of boardTokens(filter.boards)) {
        if (!boards.includes(board)) boards.push(board);
        if (boards.length > FILTER_LIMITS.boards) return { status: 'board-limit' };
      }
    }
    return { status: 'ok', boards };
  } catch { return { status: 'invalid-settings' }; }
}

function checkedJob(value) {
  if (!record(value) || value.version !== 1 || typeof value.board !== 'string'
    || value.board.length > FILTER_LIMITS.boardToken || !/^[a-z0-9]+$/.test(value.board)
    || !Array.isArray(value.posts) || value.posts.length > FILTER_LIMITS.posts
    || (value.labels !== undefined && typeof value.labels !== 'boolean')
    || (value.mode !== undefined && !['catalog', 'page'].includes(value.mode))
    || (value.thread !== undefined && typeof value.thread !== 'boolean')) throw new Error('invalid-job');
  const filters = filterRows(value.filters);
  let length = filters.reduce((sum, row) => sum + row.pattern.length + row.boards.length, 0);
  const seen = new Set();
  const posts = value.posts.map(post => {
    if (!record(post) || !canonicalId(post.no) || seen.has(post.no)) throw new Error('invalid-post');
    if (post.com !== undefined && post.comment !== undefined) throw new Error('ambiguous-comment');
    seen.add(post.no);
    const prepared = { no: post.no };
    for (const field of fields) {
      const text = post[field];
      if (text === undefined) continue;
      if (typeof text !== 'string' || text.length > (field === 'com' ? FILTER_LIMITS.html : FILTER_LIMITS.field)) throw new Error('invalid-field');
      length += text.length;
      if (length > FILTER_LIMITS.request) throw new Error('request-limit');
      prepared[field] = text;
    }
    return prepared;
  });
  return { version: 1, board: value.board, filters, posts, ...(value.labels ? { labels: true } : {}),
    ...(value.mode === 'page' ? { mode: 'page', thread: value.thread === true } : {}) };
}

function escapeNative(text, wildcards = false) {
  let output = '';
  for (const character of text) {
    if (wildcards && character === '*') output += '[^\\s]*';
    else output += (escapedCharacters.has(character) ? '\\' : '') + character;
  }
  // Deliberately do not escape |: the pinned native escaping list omits it.
  return output;
}
function compileNative(filter) {
  if (literalTypes.has(filter.type)) return filter.pattern;
  const raw = filter.pattern;
  const regular = /^\/(.*)\/(i?)$/.exec(raw);
  if (regular) return new RegExp(regular[1], regular[2]);
  if (raw[0] === '"' && raw.at(-1) === '"') return new RegExp(escapeNative(raw.slice(1, -1)));
  return new RegExp('^' + raw.split(' ').map(term => `(?=.*\\b${escapeNative(term, true)}\\b)`).join(''), 'im');
}
function matchesPrepared(filter, pattern, post) {
  if (filter.type === 0) return pattern === post.trip;
  if (filter.type === 1) return pattern === post.name;
  if (filter.type === 4) return pattern === post.id;
  // Preparation omits absent/empty raw comments, but nonempty HTML can decode to "".
  if (filter.type === 2) {
    if (post.com !== undefined) {
      if (post.com !== '') post.comment = nativeCommentText(post.com);
      delete post.com;
    }
    return post.comment !== undefined && pattern.test(post.comment);
  }
  // Native RegExp.test converts missing subject/filename fields to "undefined".
  if (filter.type === 5) return pattern.test(post.sub);
  return pattern.test(post.filename);
}

function matchesPage(filter, pattern, post, thread) {
  if (filter.type === 0) return !!post.trip && pattern === post.trip;
  if (filter.type === 1) return post.name !== undefined && pattern === post.name;
  if (filter.type === 4) return !!post.id && pattern === post.id;
  if (filter.type === 2) {
    if (post.comment === undefined) post.comment = nativeCommentText(post.com ?? '');
    return pattern.test(post.comment);
  }
  if (filter.type === 5) return !thread && !!post.sub && pattern.test(post.sub);
  return pattern.test(post.filename ?? '');
}

// Call only in a worker. The browser-facing class below never invokes this evaluator.
export function runNativeFilterJob(raw) {
  try { return evaluateNativeFilterJob(raw); }
  catch { return JSON.stringify({ status: 'invalid-request' }); }
}

function evaluateNativeFilterJob(raw) {
  let job;
  if (typeof raw !== 'string' || raw.length > FILTER_LIMITS.request) return JSON.stringify({ status: 'invalid-request' });
  try { job = checkedJob(JSON.parse(raw)); }
  catch { return JSON.stringify({ status: 'invalid-request' }); }
  const compiled = [];
  for (let index = 0; index < job.filters.length; index++) {
    const filter = job.filters[index];
    if (!filter.active || filter.pattern === '') continue;
    let pattern;
    try { pattern = compileNative(filter); }
    catch { return JSON.stringify({ status: 'invalid-filter', index }); }
    compiled.push({ filter, pattern, index, boards: boardTokens(filter.boards) });
  }
  const matches = [];
  for (const post of job.posts) {
    const rawComment = post.com;
    for (const { filter, pattern, index, boards } of compiled) {
      const scoped = boards.includes(job.board) || (job.mode === 'page' && filter.boards === '');
      if (scoped && (job.mode === 'page' ? matchesPage(filter, pattern, post, job.thread) : matchesPrepared(filter, pattern, post))) {
        matches.push({ id: post.no, filter: index,
          ...(job.labels ? { label: nativeWatchLabel({ no: post.no, sub: post.sub, com: rawComment }) } : {}) });
        break;
      }
    }
  }
  return JSON.stringify({ status: 'ok', matches });
}

function checkedResult(raw, job) {
  if (typeof raw !== 'string' || raw.length > FILTER_LIMITS.response) return { status: 'invalid-result' };
  try {
    const result = JSON.parse(raw);
    if (!record(result)) return { status: 'invalid-result' };
    if (result.status === 'invalid-request') return { status: 'invalid-request' };
    if (result.status === 'invalid-filter' && Number.isInteger(result.index)
      && result.index >= 0 && result.index < job.filters.length) return { status: 'invalid-filter', index: result.index };
    if (result.status !== 'ok' || !Array.isArray(result.matches)
      || result.matches.length > job.posts.length) return { status: 'invalid-result' };
    const positions = new Map(job.posts.map((post, index) => [post.no, index]));
    let previous = -1;
    const matches = [];
    for (const match of result.matches) {
      if (!record(match) || !canonicalId(match.id) || !positions.has(match.id)
        || !Number.isInteger(match.filter)) return { status: 'invalid-result' };
      const index = positions.get(match.id);
      const filter = job.filters[match.filter];
      if (index <= previous || !filter?.active || filter.pattern === ''
        || !(boardTokens(filter.boards).includes(job.board) || (job.mode === 'page' && filter.boards === ''))
        || (job.mode === 'page' && job.thread && filter.type === 5)) return { status: 'invalid-result' };
      if (job.labels && (typeof match.label !== 'string' || match.label.length > 45
        || /[\u0000-\u001f\u007f]/.test(match.label))) return { status: 'invalid-result' };
      previous = index;
      matches.push({ id: match.id, filter: match.filter, ...(job.labels ? { label: match.label } : {}) });
    }
    return { status: 'ok', matches };
  } catch { return { status: 'invalid-result' }; }
}

export class NativeFilterMatcher {
  constructor({ createWorker = () => new Worker(new URL(import.meta.url), { type: 'module' }),
    deadline = FILTER_LIMITS.deadline } = {}) {
    if (!Number.isInteger(deadline) || deadline < 25 || deadline > 2000) throw new RangeError('Invalid filter deadline');
    this.createWorker = createWorker;
    this.deadline = deadline;
  }

  match(filters, board, posts, { signal, labels = false, mode = 'catalog', thread = false } = {}) {
    if (signal?.aborted) return Promise.resolve({ status: 'cancelled' });
    let job, raw;
    try {
      job = checkedJob({ version: 1, filters, board, posts, labels, mode, thread });
      raw = JSON.stringify(job);
      if (raw.length > FILTER_LIMITS.request) return Promise.resolve({ status: 'invalid-request' });
    } catch { return Promise.resolve({ status: 'invalid-request' }); }
    return new Promise(resolve => {
      let worker, timer, settled = false;
      const finish = result => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        signal?.removeEventListener('abort', cancel);
        if (worker) {
          worker.onmessage = worker.onerror = null;
          try { worker.terminate(); } catch { /* No fallback evaluation on the caller's thread. */ }
        }
        resolve(result);
      };
      const cancel = () => finish({ status: 'cancelled' });
      try {
        worker = this.createWorker();
        worker.onmessage = event => finish(checkedResult(event.data, job));
        worker.onerror = event => { event.preventDefault?.(); finish({ status: 'worker-error' }); };
        timer = setTimeout(() => finish({ status: 'timeout' }), this.deadline);
        signal?.addEventListener('abort', cancel, { once: true });
        if (signal?.aborted) { cancel(); return; }
        worker.postMessage(raw);
      } catch { finish({ status: 'unavailable' }); }
    });
  }
}

if (typeof WorkerGlobalScope !== 'undefined' && globalThis instanceof WorkerGlobalScope) {
  globalThis.addEventListener('message', event => globalThis.postMessage(runNativeFilterJob(event.data)));
}
