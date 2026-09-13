import { UPDATER_LIMITS, updaterContext, updaterUrl, validatePostTree } from './native-updater-snapshot.js';
import { postId } from '../static/thread-watcher-core.v1.js';

function cancel(body) { try { body?.cancel()?.catch(() => {}); } catch { /* Closed or locked stream. */ } }
function checkedResult(result, context) {
  try {
    if (result?.status !== 'ok') return { status: 'invalid-snapshot' };
    const s = result.snapshot;
    if (s.version !== 1 || s.board !== context.board || s.thread !== context.thread
      || !['closed', 'archived', 'sticky'].every(key => typeof s[key] === 'boolean')
      || !Array.isArray(s.posts) || !s.posts.length || s.posts.length > UPDATER_LIMITS.posts
      || s.replies !== s.posts.length - 1 || !Number.isInteger(s.images) || s.images < 0 || s.images > s.replies) throw new Error();
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
  }
  cancel() { this.active?.abort(); }
  refresh({ signal } = {}) {
    if (signal?.aborted) return Promise.resolve({ status: 'cancelled' });
    if (this.active) return Promise.resolve({ status: 'busy' });
    const now = this.now();
    if (!Number.isFinite(now) || now - this.lastStarted < this.limits.intervalMs) return Promise.resolve({ status: 'cooldown' });
    if (typeof this.fetcher !== 'function') return Promise.resolve({ status: 'unavailable' });
    this.lastStarted = now;
    const controller = new AbortController(); this.active = controller;
    return new Promise(resolve => {
      let settled = false, timer, parserTimer, reader, body, worker;
      const finish = result => {
        if (settled) return;
        settled = true; clearTimeout(timer); clearTimeout(parserTimer);
        signal?.removeEventListener('abort', cancelled);
        controller.signal.removeEventListener('abort', cancelled);
        controller.abort(); cancel(reader ?? body);
        if (worker) {
          worker.onmessage = worker.onerror = null;
          try { worker.terminate(); } catch { /* Cleanup failure must still settle the request. */ }
        }
        if (this.active === controller) this.active = null;
        resolve(result);
      };
      const cancelled = () => finish({ status: 'cancelled' });
      signal?.addEventListener('abort', cancelled, { once: true });
      controller.signal.addEventListener('abort', cancelled, { once: true });
      timer = setTimeout(() => finish({ status: 'timeout' }), this.limits.requestMs);
      void (async () => {
        const response = await this.fetcher(this.url, { method: 'GET', credentials: 'omit', mode: 'same-origin',
          redirect: 'error', cache: 'no-store', headers: { Accept: 'application/json' }, signal: controller.signal });
        if (settled) { cancel(response.body); return; }
        body = response.body;
        if (response.redirected || response.url !== this.url) { finish({ status: 'invalid-response' }); return; }
        if (response.status !== 200) { finish({ status: 'http-error', httpStatus: response.status }); return; }
        if (response.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/json') {
          finish({ status: 'invalid-response' }); return;
        }
        const length = response.headers.get('content-length');
        if (length !== null && (!/^[0-9]+$/.test(length) || Number(length) > this.limits.bytes)) {
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
          bytes += part.value.byteLength;
          if (bytes > this.limits.bytes) { finish({ status: 'response-limit' }); return; }
          chunks.push(part.value);
        }
        const joined = new Uint8Array(bytes); let offset = 0;
        for (const chunk of chunks) { joined.set(chunk, offset); offset += chunk.byteLength; }
        let raw;
        try { raw = new TextDecoder('utf-8', { fatal: true }).decode(joined); }
        catch { finish({ status: 'invalid-encoding' }); return; }
        worker = this.createWorker();
        parserTimer = setTimeout(() => finish({ status: 'parse-timeout' }), this.limits.parseMs);
        worker.onmessage = event => finish(checkedResult(event.data, this.context));
        worker.onerror = event => { event.preventDefault?.(); finish({ status: 'worker-error' }); };
        worker.postMessage({ kind: 'updater-snapshot', raw, context: this.context });
      })().catch(() => finish({ status: 'network-error' }));
    });
  }
}
