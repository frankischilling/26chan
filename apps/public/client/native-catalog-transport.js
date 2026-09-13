import { WATCH_LIMITS } from '../static/thread-watcher-core.v1.js';
import { FILTER_LIMITS } from './native-filter-limits.js';
import { parseNativeCatalog } from './native-catalog.js';

export const CATALOG_TRANSPORT_LIMITS = Object.freeze({
  boards: FILTER_LIMITS.boards, responseBytes: WATCH_LIMITS.responseBytes,
  cycleBytes: WATCH_LIMITS.cycleBytes, concurrency: WATCH_LIMITS.concurrency,
  staggerMs: WATCH_LIMITS.staggerMs, requestMs: WATCH_LIMITS.requestMs,
  cycleMs: WATCH_LIMITS.cycleMs, intervalMs: WATCH_LIMITS.intervalMs,
});

function checkedOrigin(origin) {
  const url = new URL(origin);
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password
    || url.pathname !== '/' || url.search || url.hash) throw new TypeError('invalid-origin');
  return url.origin;
}

function boardToken(board) {
  return typeof board === 'string' && /^[a-zA-Z0-9]{1,32}$/.test(board);
}

export function catalogApiUrl(origin, board) {
  if (!boardToken(board)) throw new TypeError('invalid-board');
  return `${checkedOrigin(origin)}/_watch/${board}/catalog.json`;
}

function limitsWith(overrides) {
  if (!overrides || typeof overrides !== 'object' || Array.isArray(overrides)) throw new TypeError('invalid-limits');
  const limits = { ...CATALOG_TRANSPORT_LIMITS };
  for (const [key, value] of Object.entries(overrides)) {
    if (!['responseBytes', 'cycleBytes', 'concurrency', 'staggerMs', 'requestMs', 'cycleMs'].includes(key)
      || !Number.isSafeInteger(value) || value < (key === 'staggerMs' ? 0 : 1)
      || value > limits[key]) throw new RangeError('invalid-limits');
    limits[key] = value;
  }
  return Object.freeze(limits);
}

function cancelBody(body) {
  try { body?.cancel()?.catch(() => {}); } catch { /* Already closed or locked. */ }
}

function delay(ms, signal) {
  if (signal.aborted) return Promise.resolve(false);
  if (ms <= 0) return Promise.resolve(true);
  return new Promise(resolve => {
    const finish = ready => {
      clearTimeout(timer);
      signal.removeEventListener('abort', aborted);
      resolve(ready);
    };
    const aborted = () => finish(false);
    const timer = setTimeout(() => finish(true), ms);
    signal.addEventListener('abort', aborted, { once: true });
  });
}

function requestCatalog(origin, board, fetcher, limits, cycleSignal, charge) {
  const url = catalogApiUrl(origin, board);
  return new Promise(resolve => {
    const controller = new AbortController();
    let timer, body, reader, settled = false;
    const finish = result => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      cycleSignal.removeEventListener('abort', cancelled);
      controller.abort();
      cancelBody(reader ?? body);
      resolve({ board, ...result });
    };
    const cancelled = () => finish({ status: cycleSignal.reason });
    if (cycleSignal.aborted) { cancelled(); return; }
    cycleSignal.addEventListener('abort', cancelled, { once: true });
    timer = setTimeout(() => finish({ status: 'request-timeout' }), limits.requestMs);
    void (async () => {
      const response = await fetcher(url, {
        method: 'GET', credentials: 'omit', mode: 'same-origin', redirect: 'error',
        cache: 'no-store', headers: { Accept: 'application/json' }, signal: controller.signal,
      });
      if (settled) { cancelBody(response.body); return; }
      body = response.body;
      if (response.redirected || response.url !== url) { finish({ status: 'invalid-response' }); return; }
      if (response.status !== 200) { finish({ status: 'http-error', httpStatus: response.status }); return; }
      if (response.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/json') {
        finish({ status: 'invalid-mime' }); return;
      }
      const declared = response.headers.get('content-length');
      if (declared !== null && (!/^[0-9]+$/.test(declared) || Number(declared) > limits.responseBytes)) {
        finish({ status: 'response-limit' }); return;
      }
      if (!body) { finish({ status: 'invalid-catalog' }); return; }
      reader = body.getReader();
      const chunks = [];
      let length = 0;
      while (true) {
        const part = await reader.read();
        if (settled) return;
        if (part.done) break;
        if (!(part.value instanceof Uint8Array)) { finish({ status: 'invalid-response' }); return; }
        if (!charge(part.value.byteLength)) return;
        length += part.value.byteLength;
        if (length > limits.responseBytes) { finish({ status: 'response-limit' }); return; }
        chunks.push(part.value);
      }
      const bytes = new Uint8Array(length);
      let offset = 0;
      for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
      let raw;
      try { raw = new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
      catch { finish({ status: 'invalid-encoding' }); return; }
      const parsed = parseNativeCatalog(raw);
      finish(parsed.status === 'ok' ? { status: 'ok', posts: parsed.posts } : { status: 'invalid-catalog' });
    })().catch(() => finish({ status: 'network-error' }));
  });
}

export class NativeCatalogTransport {
  constructor({ origin = globalThis.location?.origin, fetcher = globalThis.fetch?.bind(globalThis),
    limits = {}, now = Date.now } = {}) {
    this.origin = checkedOrigin(origin);
    this.fetcher = fetcher;
    this.limits = limitsWith(limits);
    this.now = now;
    this.lastStarted = null;
    this.active = null;
  }

  cancel() { this.active?.abort('cancelled'); }

  async refresh(boards, { signal } = {}) {
    if (!Array.isArray(boards) || boards.length > this.limits.boards || !boards.every(boardToken)
      || new Set(boards).size !== boards.length) return { status: 'invalid-request', results: [], bytes: 0 };
    const terminal = status => ({ status, results: boards.map(board => ({ board, status })), bytes: 0 });
    if (signal?.aborted) return terminal('cancelled');
    if (!boards.length) return { status: 'complete', results: [], bytes: 0 };
    if (typeof this.fetcher !== 'function') return terminal('unavailable');
    if (this.active) return terminal('busy');
    const started = this.now();
    if (!Number.isFinite(started) || started < 0) return terminal('invalid-request');
    if (this.lastStarted !== null && started - this.lastStarted < this.limits.intervalMs) return terminal('cooldown');
    this.lastStarted = started;
    const controller = new AbortController();
    this.active = controller;
    const cancelled = () => controller.abort('cancelled');
    signal?.addEventListener('abort', cancelled, { once: true });
    const timer = setTimeout(() => controller.abort('cycle-timeout'), this.limits.cycleMs);
    const results = new Array(boards.length);
    let next = 0, bytes = 0, nextStart = 0, gate = Promise.resolve();
    const charge = count => {
      if (controller.signal.aborted) return false;
      if (bytes + count > this.limits.cycleBytes) { controller.abort('cycle-byte-limit'); return false; }
      bytes += count;
      return true;
    };
    const enter = async () => {
      const previous = gate;
      let release;
      gate = new Promise(resolve => { release = resolve; });
      try {
        await previous;
        if (!await delay(Math.max(0, nextStart - performance.now()), controller.signal)) return false;
        if (controller.signal.aborted) return false;
        nextStart = performance.now() + this.limits.staggerMs;
        return true;
      } finally { release(); }
    };
    const pump = async () => {
      while (next < boards.length) {
        const index = next++;
        const board = boards[index];
        results[index] = await enter()
          ? await requestCatalog(this.origin, board, this.fetcher, this.limits, controller.signal, charge)
          : { board, status: controller.signal.reason };
      }
    };
    try {
      await Promise.all(Array.from({ length: Math.min(boards.length, this.limits.concurrency) }, pump));
      return { status: controller.signal.aborted ? controller.signal.reason : 'complete', results, bytes };
    } finally {
      clearTimeout(timer);
      signal?.removeEventListener('abort', cancelled);
      controller.abort('cancelled');
      if (this.active === controller) this.active = null;
    }
  }
}
