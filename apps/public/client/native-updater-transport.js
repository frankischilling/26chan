import { UPDATER_LIMITS, updaterContext, updaterUrl, validatePostTree, validateSnapshotMetadata } from './native-updater-snapshot.js';
import { postId } from '../static/thread-watcher-core.v1.js';

function cancel(body) { try { body?.cancel()?.catch(() => {}); } catch { /* Closed or locked stream. */ } }
function checkedResult(result, context) {
  try {
    if (result?.status !== 'ok') return { status: 'invalid-snapshot' };
    const s = result.snapshot;
    validateSnapshotMetadata(s, context);
    let previous = 0n;
    const budget = { nodes: 0 };
    for (const post of s.posts) {
      if (postId(post.no) !== post.no || BigInt(post.no) <= previous || typeof post.file_deleted !== 'boolean') throw new Error();
      previous = BigInt(post.no);
      validatePostTree(post.tree, context, post.no, budget);
    }
    if (s.posts[0].no !== context.thread) throw new Error();
    return result;
  } catch { return { status: 'invalid-snapshot' }; }
}
function validators(response, boundary) {
  const etag = response.headers.get('etag'), modified = response.headers.get('last-modified');
  return {
    etag: typeof etag === 'string' && /^"[0-9a-f]{64}"$/.test(etag) ? etag : null,
    modified: typeof modified === 'string' && modified.length === 29
      && /^[A-Za-z]{3}, [0-9]{2} [A-Za-z]{3} [0-9]{4} [0-9]{2}:[0-9]{2}:[0-9]{2} GMT$/.test(modified)
      && Number.isFinite(Date.parse(modified)) ? modified : null,
    boundary,
  };
}

export class NativeUpdaterTransport {
  constructor({ origin = globalThis.location?.origin, board, thread, mediaOrigin = '',
    fetcher = globalThis.fetch?.bind(globalThis),
    createWorker = () => new Worker(new URL(import.meta.url), { type: 'module' }),
    now = () => performance.now(), limits = {} }) {
    this.context = updaterContext({ origin, board, thread, mediaOrigin });
    this.url = updaterUrl(this.context);
    this.fetcher = fetcher; this.createWorker = createWorker; this.now = now;
    this.limits = { ...UPDATER_LIMITS };
    for (const [key, value] of Object.entries(limits)) {
      if (!['bytes', 'requestMs', 'parseMs'].includes(key) || !Number.isInteger(value) || value < 1 || value > this.limits[key]) throw new RangeError('invalid-limits');
      this.limits[key] = value;
    }
    this.active = null; this.lastStarted = -Infinity;
    this.validators = { full: null, tail: null };
  }
  cancel() { this.active?.abort(); this.invalidate(); }
  invalidate() { this.validators = { full: null, tail: null }; }
  refresh({ signal, tail = false, known = new Set() } = {}) {
    if (signal?.aborted) return Promise.resolve({ status: 'cancelled' });
    if (typeof tail !== 'boolean' || !(known instanceof Set) || known.size > UPDATER_LIMITS.posts
      || [...known].some(id => postId(id) !== id) || (tail && !known.has(this.context.thread))) return Promise.resolve({ status: 'invalid-context' });
    known = new Set(known);
    if (this.active) return Promise.resolve({ status: 'busy' });
    const now = this.now();
    if (!Number.isFinite(now) || now - this.lastStarted < this.limits.intervalMs) return Promise.resolve({ status: 'cooldown' });
    if (typeof this.fetcher !== 'function') return Promise.resolve({ status: 'unavailable' });
    this.lastStarted = now;
    const controller = new AbortController(); this.active = controller;
    return new Promise(resolve => {
      let settled = false, timer, parserTimer, reader, body, worker;
      const disposeWorker = () => {
        clearTimeout(parserTimer);
        if (worker) {
          worker.onmessage = worker.onerror = null;
          try { worker.terminate(); } catch { /* Cleanup failure must still settle the request. */ }
          worker = null;
        }
      };
      const finish = result => {
        if (settled) return;
        settled = true; clearTimeout(timer); disposeWorker();
        signal?.removeEventListener('abort', cancelled);
        controller.signal.removeEventListener('abort', cancelled);
        controller.abort(); cancel(reader ?? body);
        if (this.active === controller) this.active = null;
        resolve(result);
      };
      const cancelled = () => finish({ status: 'cancelled' });
      signal?.addEventListener('abort', cancelled, { once: true });
      controller.signal.addEventListener('abort', cancelled, { once: true });
      timer = setTimeout(() => finish({ status: 'timeout' }), this.limits.requestMs);
      void (async () => {
        let used = 0;
        // One tail attempt may fall back once. Both share the same deadline,
        // cancellation signal, byte budget and outer refresh slot.
        for (let attempt = 0; attempt < (tail ? 2 : 1); attempt++) {
          const isTail = tail && attempt === 0, mode = isTail ? 'tail' : 'full';
          const url = updaterUrl(this.context, isTail);
          let cached = this.validators[mode];
          if (isTail && cached && !known.has(cached.boundary)) cached = null;
          const headers = { Accept: 'application/json', 'If-Modified-Since': cached?.modified ?? '0' };
          if (cached?.etag) headers['If-None-Match'] = cached.etag;
          const response = await this.fetcher(url, { method: 'GET', credentials: 'omit', mode: 'same-origin',
            redirect: 'error', cache: 'no-store', headers, signal: controller.signal });
          if (settled) { cancel(response.body); return; }
          body = response.body; reader = null;
          if (response.redirected || response.url !== url) { finish({ status: 'invalid-response' }); return; }
          if (isTail && response.status === 404) { this.validators.tail = null; cancel(body); body = null; continue; }
          if (response.status === 304) {
            finish(cached?.etag || cached?.modified ? { status: 'not-modified' } : { status: 'invalid-response' }); return;
          }
          if (response.status !== 200) { finish({ status: 'http-error', httpStatus: response.status }); return; }
          if (response.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/json') {
            finish({ status: 'invalid-response' }); return;
          }
          const length = response.headers.get('content-length');
          if (length !== null && (!/^[0-9]+$/.test(length) || Number(length) > this.limits.bytes - used)) {
            finish({ status: 'response-limit' }); return;
          }
          if (!body) { finish({ status: 'invalid-response' }); return; }
          reader = body.getReader();
          const chunks = []; let bytes = 0;
          while (true) {
            const part = await reader.read();
            if (settled) return;
            if (part.done) break;
            if (!(part.value instanceof Uint8Array)) { finish({ status: 'invalid-response' }); return; }
            bytes += part.value.byteLength; used += part.value.byteLength;
            if (used > this.limits.bytes) { finish({ status: 'response-limit' }); return; }
            chunks.push(part.value);
          }
          const joined = new Uint8Array(bytes); let offset = 0;
          for (const chunk of chunks) { joined.set(chunk, offset); offset += chunk.byteLength; }
          let raw;
          try { raw = new TextDecoder('utf-8', { fatal: true }).decode(joined); }
          catch { finish({ status: 'invalid-encoding' }); return; }
          const result = await new Promise(parsed => {
            worker = this.createWorker();
            parserTimer = setTimeout(() => finish({ status: 'parse-timeout' }), this.limits.parseMs);
            worker.onmessage = event => { const value = checkedResult(event.data, this.context); disposeWorker(); parsed(value); };
            worker.onerror = event => { event.preventDefault?.(); disposeWorker(); parsed({ status: 'worker-error' }); };
            worker.postMessage({ kind: 'updater-snapshot', raw, context: this.context });
          });
          if (settled) return;
          if (result.status !== 'ok') { finish(result); return; }
          if (isTail && !known.has(result.snapshot.tail_id)) {
            this.validators.tail = null; reader = null; body = null; continue;
          }
          if (!isTail && result.snapshot.tail_id !== null) { finish({ status: 'invalid-snapshot' }); return; }
          this.validators[mode] = validators(response, result.snapshot.tail_id);
          finish(result); return;
        }
        finish({ status: 'invalid-snapshot' });
      })().catch(() => finish({ status: 'network-error' }));
    });
  }
}
