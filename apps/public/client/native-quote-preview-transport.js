import { PREVIEW_LIMITS, previewContext, previewUrl, validatePreviewMetadata, validatePostTree } from './native-updater-snapshot.js';

function cancel(body) { try { body?.cancel()?.catch(() => {}); } catch { /* A closed stream needs no further cleanup. */ } }

export function checkedQuotePreview(result, context) {
  try {
    if (result?.status !== 'ok') throw new TypeError('invalid-preview');
    const resolved = validatePreviewMetadata(result.snapshot, context, true);
    validatePostTree(result.snapshot.post.tree, resolved, context.post, { nodes: 0 }, PREVIEW_LIMITS);
    return { status: 'ok', snapshot: result.snapshot, context: resolved };
  } catch { return { status: 'invalid-preview' }; }
}

// One request and one disposable parser at a time. No positive or negative cache:
// every new hover can observe a deletion, with a short global request cooldown.
export class NativeQuotePreviewTransport {
  constructor({ origin = globalThis.location?.origin, mediaOrigin = '',
    fetcher = globalThis.fetch?.bind(globalThis),
    createWorker = () => new Worker(new URL(import.meta.url), { type: 'module' }),
    now = () => performance.now(), limits = {} } = {}) {
    const context = previewContext({ origin, mediaOrigin, board: 'a', post: '1' });
    this.origin = context.origin; this.mediaOrigin = context.mediaOrigin;
    this.fetcher = fetcher; this.createWorker = createWorker; this.now = now;
    this.limits = { ...PREVIEW_LIMITS };
    for (const [key, value] of Object.entries(limits)) {
      if (!['bytes', 'requestMs', 'parseMs'].includes(key) || !Number.isInteger(value)
        || value < 1 || value > this.limits[key]) throw new RangeError('invalid-limits');
      this.limits[key] = value;
    }
    this.active = null; this.lastStarted = -Infinity;
  }

  cancel() { this.active?.abort(); }

  load(target, { signal } = {}) {
    if (signal?.aborted) return Promise.resolve({ status: 'cancelled' });
    let context, url;
    try {
      context = previewContext({ ...target, origin: this.origin, mediaOrigin: this.mediaOrigin });
      url = previewUrl(context);
    } catch { return Promise.resolve({ status: 'invalid-context' }); }
    if (this.active) return Promise.resolve({ status: 'busy' });
    const now = this.now(), delay = this.limits.intervalMs - (now - this.lastStarted);
    if (!Number.isFinite(now)) return Promise.resolve({ status: 'unavailable' });
    if (delay > 0) return Promise.resolve({ status: 'cooldown', retryAfter: Math.ceil(delay) });
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
        if (worker) {
          worker.onmessage = worker.onerror = null;
          try { worker.terminate(); } catch { /* Termination cannot prevent settlement. */ }
        }
        controller.abort(); cancel(reader ?? body);
        if (this.active === controller) this.active = null;
        resolve(result);
      };
      const cancelled = () => finish({ status: 'cancelled' });
      signal?.addEventListener('abort', cancelled, { once: true });
      controller.signal.addEventListener('abort', cancelled, { once: true });
      timer = setTimeout(() => finish({ status: 'timeout' }), this.limits.requestMs);
      if (signal?.aborted) { cancelled(); return; }
      void (async () => {
        const response = await this.fetcher(url, { method: 'GET', credentials: 'omit', mode: 'same-origin',
          redirect: 'error', cache: 'no-store', headers: { Accept: 'application/json' }, signal: controller.signal });
        if (settled) { cancel(response.body); return; }
        body = response.body;
        if (response.redirected || response.url !== url) { finish({ status: 'invalid-response' }); return; }
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
        const chunks = []; let bytes = 0, reads = 0;
        while (true) {
          const part = await reader.read();
          if (settled) return;
          if (part.done) break;
          if (++reads > this.limits.bytes) { finish({ status: 'response-limit' }); return; }
          if (!(part.value instanceof Uint8Array)) { finish({ status: 'invalid-response' }); return; }
          bytes += part.value.byteLength;
          if (bytes > this.limits.bytes) { finish({ status: 'response-limit' }); return; }
          if (part.value.byteLength) chunks.push(part.value);
        }
        const joined = new Uint8Array(bytes); let offset = 0;
        for (const chunk of chunks) { joined.set(chunk, offset); offset += chunk.byteLength; }
        let raw;
        try { raw = new TextDecoder('utf-8', { fatal: true }).decode(joined); }
        catch { finish({ status: 'invalid-encoding' }); return; }
        worker = this.createWorker();
        parserTimer = setTimeout(() => finish({ status: 'parse-timeout' }), this.limits.parseMs);
        worker.onmessage = event => finish(checkedQuotePreview(event.data, context));
        worker.onerror = event => { event.preventDefault?.(); finish({ status: 'worker-error' }); };
        worker.postMessage({ kind: 'quote-preview', raw, context });
      })().catch(() => finish({ status: 'network-error' }));
    });
  }
}
