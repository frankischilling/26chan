import { boardPageContext, validateBoardPageSnapshot } from '../static/native-filter.v1.js';

const MAX_BYTES = 4194304;
function cancel(body) { try { body?.cancel()?.catch(() => {}); } catch { /* An already closed stream needs no action. */ } }

export class NativeBoardPageTransport {
  constructor({ origin = globalThis.location?.origin, board, mediaOrigin = '',
    fetcher = globalThis.fetch?.bind(globalThis),
    createWorker = () => new Worker('/static/native-filter.v1.js', { type: 'module' }), limits = {} }) {
    this.context = boardPageContext({ origin, board, mediaOrigin, page: 0 });
    this.fetcher = fetcher; this.createWorker = createWorker; this.active = null;
    this.limits = { bytes: MAX_BYTES, requestMs: 10000, parseMs: 2000 };
    for (const [key, value] of Object.entries(limits)) {
      if (!(key in this.limits) || !Number.isInteger(value) || value < 1 || value > this.limits[key]) throw new RangeError('page-transport-limit');
      this.limits[key] = value;
    }
  }
  cancel() { this.active?.abort(); }
  refresh({ page, signal } = {}) {
    let context;
    try { context = boardPageContext({ ...this.context, page }); } catch { return Promise.resolve({ status: 'invalid-context' }); }
    if (signal?.aborted) return Promise.resolve({ status: 'cancelled' });
    if (this.active) return Promise.resolve({ status: 'busy' });
    if (typeof this.fetcher !== 'function') return Promise.resolve({ status: 'unavailable' });
    const controller = new AbortController(); this.active = controller;
    const url = `${context.origin}/_watch/${context.board}/page/${page}`;
    return new Promise(resolve => {
      let settled = false, timer, parseTimer, reader, body, worker;
      const disposeWorker = () => {
        clearTimeout(parseTimer);
        if (worker) { worker.onmessage = worker.onerror = null; try { worker.terminate(); } catch { /* Still settle. */ } worker = null; }
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
        const response = await this.fetcher(url, { method: 'GET', credentials: 'omit', mode: 'same-origin',
          redirect: 'error', cache: 'no-store', headers: { Accept: 'application/json' }, signal: controller.signal });
        if (settled) { cancel(response.body); return; }
        body = response.body;
        if (response.redirected || response.url !== url) { finish({ status: 'invalid-response' }); return; }
        if (response.status !== 200) { finish({ status: 'http-error', httpStatus: response.status }); return; }
        const declared = response.headers.get('content-length');
        if (response.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/json') {
          finish({ status: 'invalid-response' }); return;
        }
        if (declared !== null && (!/^\d+$/.test(declared) || Number(declared) > this.limits.bytes)) {
          finish({ status: 'response-limit' }); return;
        }
        if (!body) { finish({ status: 'invalid-response' }); return; }
        reader = body.getReader();
        let size = 0, reads = 0;
        const chunks = [];
        for (;;) {
          const part = await reader.read();
          if (settled) return;
          if (part.done) break;
          if (!(part.value instanceof Uint8Array)) { finish({ status: 'invalid-response' }); return; }
          size += part.value.byteLength;
          if (size > this.limits.bytes || ++reads > 65536) { finish({ status: 'response-limit' }); return; }
          chunks.push(part.value);
        }
        const bytes = new Uint8Array(size); let offset = 0;
        for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
        let raw;
        try { raw = new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
        catch { finish({ status: 'invalid-encoding' }); return; }
        worker = this.createWorker();
        parseTimer = setTimeout(() => finish({ status: 'parse-timeout' }), this.limits.parseMs);
        worker.onmessage = event => {
          try {
            if (event.data?.status !== 'ok') throw new TypeError('page-parser-error');
            const snapshot = validateBoardPageSnapshot(event.data.snapshot, context);
            finish({ status: 'ok', snapshot });
          } catch { finish({ status: 'invalid-snapshot' }); }
        };
        worker.onerror = event => { event.preventDefault?.(); finish({ status: 'worker-error' }); };
        worker.postMessage({ kind: 'board-page-snapshot', raw, context });
      })().catch(() => finish({ status: 'network-error' }));
    });
  }
}
