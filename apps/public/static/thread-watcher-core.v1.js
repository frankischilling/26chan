// Shared watcher state and transport. No DOM, stored URLs, or application credentials.
export const WATCH_LIMITS = Object.freeze({
  entries: 128,
  storageChars: 65536,
  trackedPosts: 512,
  trackedChars: 16384,
  labelChars: 45,
  posts: 20001,
  commentChars: 131072,
  responseBytes: 4 * 1024 * 1024,
  cycleBytes: 16 * 1024 * 1024,
  concurrency: 2,
  staggerMs: 200,
  requestMs: 10000,
  cycleMs: 60000,
  intervalMs: 60000,
});

const MAX_ID = 9223372036854775807n;
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);

export function postId(value, zero = false) {
  if (typeof value === 'number') {
    if (!Number.isSafeInteger(value)) return null;
    value = String(value);
  }
  if (typeof value !== 'string' || !/^(?:0|[1-9][0-9]{0,18})$/.test(value)) return null;
  if ((!zero && value === '0') || BigInt(value) > MAX_ID) return null;
  return value;
}

export function watchKey(board, id) {
  const number = postId(id);
  return typeof board === 'string' && /^[a-z0-9]{1,16}$/.test(board) && number
    ? `${number}-${board}` : null;
}

export function splitWatchKey(key) {
  if (typeof key !== 'string') return null;
  const match = /^([1-9][0-9]{0,18})-([a-z0-9]{1,16})$/.exec(key);
  return match && watchKey(match[2], match[1]) ? { id: match[1], board: match[2] } : null;
}

export function watchLabel(subject, comment, id) {
  // Callers supply text, not HTML. Preserve the reference's UTF-16 length rule.
  const text = typeof subject === 'string' && subject ? subject
    : typeof comment === 'string' && comment ? comment : `No.${postId(id) || ''}`;
  return text.replace(/[\u0000-\u001f\u007f]/g, ' ').slice(0, WATCH_LIMITS.labelChars);
}

function flag(value) {
  if (value === undefined || value === 0 || value === false) return false;
  if (value === 1 || value === true) return true;
  return null;
}

function entryFromTuple(tuple) {
  if (!Array.isArray(tuple) || tuple.length < 3 || tuple.length > 5) return null;
  const [label, position, unread] = tuple;
  const read = position === -1 ? '-1' : postId(position, true);
  const archived = flag(tuple[3]);
  const ownReply = flag(tuple[4]);
  if (typeof label !== 'string' || label.length > WATCH_LIMITS.labelChars || read === null
    || !Number.isSafeInteger(unread) || unread < 0 || unread >= WATCH_LIMITS.posts
    || archived === null || ownReply === null) return null;
  return Object.freeze({ label: label.replace(/[\u0000-\u001f\u007f]/g, ' '), read, unread, archived, ownReply });
}

export function readWatches(raw) {
  const result = new Map();
  if (typeof raw !== 'string' || raw.length > WATCH_LIMITS.storageChars) return result;
  try {
    const parsed = JSON.parse(raw);
    if (!object(parsed)) return result;
    const keys = Object.keys(parsed);
    if (keys.length > WATCH_LIMITS.entries) return result;
    for (const key of keys) {
      const entry = splitWatchKey(key) && entryFromTuple(parsed[key]);
      if (entry) result.set(key, entry);
    }
  } catch { /* Optional storage may be absent or malformed. */ }
  return result;
}

function entryTuple(entry) {
  const read = entry.read === '-1' ? -1 : postId(entry.read, true);
  if (read === null) throw new Error('Invalid watch position');
  const position = read === -1 ? -1
    : BigInt(read) <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(read) : read;
  const tuple = [entry.label, position, entry.unread, entry.archived, entry.ownReply];
  if (!entryFromTuple(tuple)) throw new Error('Invalid watch entry');
  return [entry.label, position, entry.unread, entry.archived ? 1 : 0, entry.ownReply ? 1 : 0];
}

export function writeWatches(entries) {
  if (!(entries instanceof Map) || entries.size > WATCH_LIMITS.entries) throw new Error('Watch limit');
  const value = Object.create(null);
  for (const [key, entry] of entries) {
    if (!splitWatchKey(key)) throw new Error('Invalid watch key');
    value[key] = entryTuple(entry);
  }
  const raw = JSON.stringify(value);
  if (raw.length > WATCH_LIMITS.storageChars) throw new Error('Watch storage limit');
  return raw;
}

export function sameEntry(left, right) {
  return !!left && !!right && left.label === right.label && left.read === right.read
    && left.unread === right.unread && left.archived === right.archived && left.ownReply === right.ownReply;
}

export function orderedWatches(entries) {
  // Array.sort is stable: insertion order is retained within each board.
  return [...entries].sort(([left], [right]) => {
    const a = splitWatchKey(left)?.board || '';
    const b = splitWatchKey(right)?.board || '';
    return a < b ? -1 : a > b ? 1 : 0;
  });
}

export function readTrackedReplies(raw) {
  const result = new Set();
  if (typeof raw !== 'string' || raw.length > WATCH_LIMITS.trackedChars) return result;
  try {
    const parsed = JSON.parse(raw);
    if (!object(parsed) || Object.keys(parsed).length > WATCH_LIMITS.trackedPosts) return result;
    for (const [key, value] of Object.entries(parsed)) {
      const id = key.startsWith('>>') && postId(key.slice(2));
      if (id && value === 1) result.add(id);
    }
  } catch { /* Tracking is an optional local hint, never proof of authorship. */ }
  return result;
}

export function autoRefreshEligible(raw, catalog, now = Date.now()) {
  const valid = typeof raw === 'string' && /^[0-9]{1,16}$/.test(raw);
  const timestamp = valid ? Number(raw) : NaN;
  if (!Number.isSafeInteger(timestamp) || timestamp < 0 || timestamp > now) return !catalog;
  return now - timestamp >= WATCH_LIMITS.intervalMs;
}

function exactJsonId(value, context) {
  if (typeof value !== 'number') throw new Error('JSON post IDs must be numbers');
  // Modern JSON.parse exposes the original token. Older engines may only accept
  // safe integers; they must never silently round an i64 identifier.
  const id = postId(context?.source ?? value, true);
  if (id === null) throw new Error('Invalid JSON post ID');
  return id;
}

export function parseThread(raw, expectedId) {
  const id = postId(expectedId);
  if (!id || typeof raw !== 'string' || raw.length > WATCH_LIMITS.responseBytes) throw new Error('Invalid thread');
  const value = JSON.parse(raw, (key, value, context) => key === 'no' || key === 'resto'
    ? exactJsonId(value, context) : value);
  if (!object(value) || !Array.isArray(value.posts) || !value.posts.length
    || value.posts.length > WATCH_LIMITS.posts) throw new Error('Invalid post collection');
  let previous = 0n;
  const posts = value.posts.map((post, index) => {
    if (!object(post) || !postId(post.no) || post.resto !== (index === 0 ? '0' : id)
      || (index === 0 && post.no !== id) || BigInt(post.no) <= previous
      || (post.com !== undefined && (typeof post.com !== 'string' || post.com.length > WATCH_LIMITS.commentChars))) {
      throw new Error('Invalid thread post');
    }
    previous = BigInt(post.no);
    return Object.freeze({ id: post.no, comment: post.com || '' });
  });
  const archived = flag(value.posts[0].archived);
  if (archived === null) throw new Error('Invalid archive flag');
  return Object.freeze({ posts: Object.freeze(posts), archived });
}

export function quotedPostIds(comment) {
  const result = new Set();
  if (typeof comment !== 'string' || comment.length > WATCH_LIMITS.commentChars) return result;
  // Inspect bounded anchor text only. Never instantiate untrusted HTML, images,
  // scripts, attributes or URLs. Reference own-reply matching uses quote text.
  const anchors = /<a\b([^<>]{0,1024})>([^<>]{0,128})<\/a\s*>/gi;
  for (const match of comment.matchAll(anchors)) {
    const classes = /(?:^|\s)class\s*=\s*(?:"([^"]*)"|'([^']*)')/i.exec(match[1]);
    if (!classes || !(classes[1] ?? classes[2]).split(/\s+/).includes('quotelink')) continue;
    const text = match[2].replace(/&gt;|&#0*62;|&#x0*3e;/gi, '>');
    const id = text.startsWith('>>') && postId(text.slice(2));
    if (id) result.add(id);
  }
  return result;
}

export function refreshedEntry(entry, thread, tracked = new Set()) {
  if (entry.read === '-1') throw new Error('Dead watches must be removed, not fetched');
  const read = BigInt(entry.read);
  let unread = 0;
  let ownReply = entry.ownReply;
  for (const post of thread.posts.slice(1)) {
    if (BigInt(post.id) <= read) continue;
    unread += 1;
    if (!ownReply && tracked.size) {
      for (const id of quotedPostIds(post.comment)) {
        if (tracked.has(id)) { ownReply = true; break; }
      }
    }
  }
  return Object.freeze({ ...entry, unread: Math.max(entry.unread, unread),
    archived: entry.archived || thread.archived, ownReply });
}

export function acknowledgedEntry(entry, position, advanceOnly = true) {
  const read = postId(position);
  if (!read) throw new Error('Invalid read position');
  return Object.freeze({ ...entry,
    read: advanceOnly && BigInt(entry.read) > BigInt(read) ? entry.read : read,
    unread: 0, ownReply: false });
}

export function threadApiUrl(origin, key) {
  const parts = splitWatchKey(key);
  const base = new URL(origin);
  if (!parts || !['http:', 'https:'].includes(base.protocol) || base.username || base.password
    || base.pathname !== '/' || base.search || base.hash) throw new Error('Invalid watcher origin or key');
  return `${base.origin}/_watch/${parts.board}/thread/${parts.id}.json`;
}

function wait(ms, signal) {
  if (signal.aborted) return Promise.reject(signal.reason);
  if (ms <= 0) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const abort = () => { clearTimeout(timer); reject(signal.reason); };
    const timer = setTimeout(() => { signal.removeEventListener('abort', abort); resolve(); }, ms);
    signal.addEventListener('abort', abort, { once: true });
  });
}

function abandonBody(body) {
  try { body?.cancel()?.catch(() => {}); } catch { /* Already closed or locked. */ }
}

function abortable(operation, signal, late = () => {}) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const finish = (callback, value) => {
      if (settled) return;
      settled = true;
      signal.removeEventListener('abort', cancelled);
      callback(value);
    };
    const cancelled = () => finish(reject, signal.reason);
    if (signal.aborted) { cancelled(); return; }
    signal.addEventListener('abort', cancelled, { once: true });
    Promise.resolve().then(() => {
      if (signal.aborted) throw signal.reason;
      return operation();
    }).then(value => {
      if (settled) late(value); else finish(resolve, value);
    }, error => finish(reject, error));
  });
}

async function fetchThread(url, fetcher, signal, limits, budget) {
  const request = new AbortController();
  const abort = () => request.abort(signal.reason);
  signal.addEventListener('abort', abort, { once: true });
  if (signal.aborted) abort();
  const timer = setTimeout(() => request.abort(new Error('Watcher request timed out')), limits.requestMs);
  let reader;
  let response;
  try {
    response = await abortable(() => fetcher(url, { method: 'GET', mode: 'same-origin', credentials: 'omit',
      redirect: 'error', cache: 'no-store', referrerPolicy: 'no-referrer', signal: request.signal }),
    request.signal, response => abandonBody(response?.body));
    if (request.signal.aborted) throw request.signal.reason;
    if (response.url !== url || response.redirected) throw new Error('Unexpected watcher response URL');
    if (response.status === 404) return null;
    if (response.status !== 200 || !/^application\/json(?:\s*;|$)/i.test(response.headers.get('content-type') || '')) {
      throw new Error('Thread refresh failed');
    }
    const length = response.headers.get('content-length');
    if (length !== null && (!/^[0-9]+$/.test(length) || Number(length) > limits.responseBytes)) {
      throw new Error('Thread response exceeds its byte budget');
    }
    reader = response.body?.getReader();
    if (!reader) throw new Error('Missing thread body');
    const decoder = new TextDecoder('utf-8', { fatal: true });
    let size = 0;
    let raw = '';
    for (;;) {
      const chunk = await abortable(() => reader.read(), request.signal);
      if (request.signal.aborted) throw request.signal.reason;
      if (chunk.done) break;
      size += chunk.value.byteLength;
      budget.bytes -= chunk.value.byteLength;
      if (size > limits.responseBytes || budget.bytes < 0) throw new Error('Watcher byte budget exhausted');
      raw += decoder.decode(chunk.value, { stream: true });
    }
    return raw + decoder.decode();
  } finally {
    clearTimeout(timer);
    signal.removeEventListener('abort', abort);
    request.abort();
    abandonBody(reader ?? response?.body);
  }
}

export class WatcherRefresh {
  #generation = 0;
  #controller = null;
  #lastStart = -Infinity;

  constructor({ origin, getEntries, commit, getTracked = () => new Set(), fetcher = fetch,
    now = Date.now, limits = WATCH_LIMITS }) {
    // Limits are release/test configuration, never read from local storage.
    threadApiUrl(origin, '1-test');
    this.origin = origin;
    this.getEntries = getEntries;
    this.commit = commit;
    this.getTracked = getTracked;
    this.fetcher = fetcher;
    this.now = now;
    this.limits = limits;
  }

  cancel() {
    this.#generation += 1;
    this.#controller?.abort(new Error('Watcher refresh cancelled'));
    this.#controller = null;
  }

  async refresh({ signal: outerSignal, bytes = this.limits.cycleBytes } = {}) {
    if (!Number.isSafeInteger(bytes) || bytes < 0 || bytes > this.limits.cycleBytes) return { status: 'invalid-budget', results: [] };
    if (outerSignal?.aborted) return { status: 'cancelled', results: [] };
    const now = this.now();
    if (now - this.#lastStart < this.limits.intervalMs) return { status: 'cooldown', results: [] };
    const entries = [...readWatches(writeWatches(this.getEntries()))];
    this.cancel();
    this.#lastStart = now;
    const generation = this.#generation;
    const controller = this.#controller = new AbortController();
    const { signal } = controller;
    const abort = () => controller.abort(outerSignal.reason);
    outerSignal?.addEventListener('abort', abort, { once: true });
    if (outerSignal?.aborted) abort();
    const current = () => !signal.aborted && generation === this.#generation;
    const timer = setTimeout(() => controller.abort(new Error('Watcher cycle timed out')), this.limits.cycleMs);
    const budget = { bytes };
    const results = new Array(entries.length);
    let cursor = 0;
    let nextStart = now;
    const worker = async () => {
      while (cursor < entries.length) {
        const index = cursor++;
        const [key, expected] = entries[index];
        if (!current()) { results[index] = { key, status: 'cancelled' }; continue; }
        try {
          let next = null;
          let status = 'removed';
          if (expected.read !== '-1') {
            if (budget.bytes <= 0) throw new Error('Watcher byte budget exhausted');
            const start = Math.max(nextStart, this.now());
            nextStart = start + this.limits.staggerMs;
            await wait(start - this.now(), signal);
            if (!current()) throw signal.reason;
            if (budget.bytes <= 0) throw new Error('Watcher byte budget exhausted');
            const raw = await fetchThread(threadApiUrl(this.origin, key), this.fetcher, signal, this.limits, budget);
            if (!current()) throw signal.reason;
            next = raw === null ? Object.freeze({ ...expected, read: '-1' })
              : refreshedEntry(expected, parseThread(raw, splitWatchKey(key).id), this.getTracked(key));
            status = raw === null ? 'dead' : 'updated';
          }
          if (!current()) throw signal.reason;
          // The integration must compare expected against freshly loaded state
          // inside its storage lock, and check signal before writing. A delayed
          // response must not undo unwatch, acknowledgement, or another tab's edit.
          const applied = await this.commit(key, expected, next, signal);
          results[index] = { key, status: current() ? applied ? status : 'stale' : 'cancelled' };
        } catch {
          results[index] = { key, status: current() ? 'failed' : 'cancelled' };
        }
      }
    };
    try {
      await Promise.all(Array.from({ length: Math.min(this.limits.concurrency, entries.length) }, worker));
      return { status: current() ? 'complete' : 'cancelled', results };
    } finally {
      clearTimeout(timer);
      outerSignal?.removeEventListener('abort', abort);
      if (generation === this.#generation) this.#controller = null;
    }
  }
}
