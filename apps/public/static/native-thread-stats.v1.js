const MAX_ID = 9223372036854775807n;

export const THREAD_STATS_LIMITS = Object.freeze({
  bytes: 4096,
  requestMs: 5000,
  pollMs: 180000,
});

function threadId(value) {
  return typeof value === 'string'
    && /^[1-9][0-9]{0,18}$/.test(value)
    && BigInt(value) <= MAX_ID
    ? value : null;
}

export function threadStatsContext({ origin = globalThis.location?.origin, board, thread } = {}) {
  if (typeof origin !== 'string' || typeof board !== 'string' || !/^[a-z0-9]{1,10}$/.test(board)
    || typeof thread !== 'string' || threadId(thread) !== thread) throw new TypeError('invalid-thread-stats-context');
  let parsed;
  try { parsed = new URL(origin); } catch { throw new TypeError('invalid-thread-stats-context'); }
  if (!['http:', 'https:'].includes(parsed.protocol) || parsed.origin !== origin
    || parsed.username || parsed.password || parsed.pathname !== '/' || parsed.search || parsed.hash) {
    throw new TypeError('invalid-thread-stats-context');
  }
  return Object.freeze({ origin, board, thread });
}

export function threadStatsApiUrl(context) {
  const checked = threadStatsContext(context);
  return `${checked.origin}/_watch/${checked.board}/thread/${checked.thread}/stats`;
}

function exactObject(value, keys) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).sort().join(',') === [...keys].sort().join(',');
}

export function parseThreadStats(raw, context) {
  const checked = threadStatsContext(context);
  if (typeof raw !== 'string' || raw.length > THREAD_STATS_LIMITS.bytes
    || new TextEncoder().encode(raw).length > THREAD_STATS_LIMITS.bytes) throw new TypeError('invalid-thread-stats');
  let value;
  try { value = JSON.parse(raw); } catch { throw new TypeError('invalid-thread-stats'); }
  const keys = ['version', 'board', 'thread', 'replies', 'images', 'sticky', 'closed', 'archived',
    'bump_limited', 'image_limited', 'page'];
  if (!exactObject(value, keys) || value.version !== 1 || value.board !== checked.board
    || value.thread !== checked.thread || !Number.isInteger(value.replies) || value.replies < 0
    || value.replies > 1000 || !Number.isInteger(value.images) || value.images < 0
    || value.images > value.replies
    || ['sticky', 'closed', 'archived', 'bump_limited', 'image_limited'].some(key => typeof value[key] !== 'boolean')
    || (value.archived ? value.page !== null
      : !Number.isInteger(value.page) || value.page < 1 || value.page > 1000)) {
    throw new TypeError('invalid-thread-stats');
  }
  return Object.freeze({ ...value });
}

function cancelBody(body) {
  try { body?.cancel?.().catch?.(() => {}); } catch { /* best effort */ }
}

export class NativeThreadStatsTransport {
  constructor({ origin = globalThis.location?.origin, board, thread,
    fetcher = globalThis.fetch?.bind(globalThis), limits = {} } = {}) {
    this.context = threadStatsContext({ origin, board, thread });
    this.url = threadStatsApiUrl(this.context);
    this.fetcher = fetcher;
    this.limits = { bytes: THREAD_STATS_LIMITS.bytes, requestMs: THREAD_STATS_LIMITS.requestMs };
    for (const [name, value] of Object.entries(limits)) {
      if (!(name in this.limits) || !Number.isInteger(value) || value < 1 || value > this.limits[name]) {
        throw new RangeError('invalid-thread-stats-limit');
      }
      this.limits[name] = value;
    }
    this.active = null;
  }

  cancel() { this.active?.abort(); }

  load({ signal } = {}) {
    if (signal?.aborted) return Promise.resolve({ status: 'cancelled' });
    if (this.active) return Promise.resolve({ status: 'busy' });
    if (typeof this.fetcher !== 'function') return Promise.resolve({ status: 'unavailable' });
    const controller = new AbortController();
    this.active = controller;
    return new Promise(resolve => {
      let done = false, timedOut = false, timer = null, reader = null, body = null;
      const finish = result => {
        if (done) return;
        done = true;
        if (timer !== null) clearTimeout(timer);
        signal?.removeEventListener('abort', externalAbort);
        controller.signal.removeEventListener('abort', cancelled);
        try { reader?.cancel?.().catch?.(() => {}); } catch { /* best effort */ }
        if (!reader) cancelBody(body);
        if (!controller.signal.aborted) controller.abort();
        if (this.active === controller) this.active = null;
        resolve(result);
      };
      const cancelled = () => finish({ status: timedOut ? 'timeout' : 'cancelled' });
      const externalAbort = () => controller.abort();
      controller.signal.addEventListener('abort', cancelled, { once: true });
      signal?.addEventListener('abort', externalAbort, { once: true });
      timer = setTimeout(() => { timedOut = true; controller.abort(); }, this.limits.requestMs);
      (async () => {
        const response = await this.fetcher(this.url, {
          method: 'GET',
          credentials: 'omit',
          mode: 'same-origin',
          redirect: 'error',
          cache: 'no-store',
          headers: { Accept: 'application/json' },
          signal: controller.signal,
        });
        if (done) { cancelBody(response?.body); return; }
        body = response?.body;
        if (!response || response.redirected || response.url !== this.url) {
          finish({ status: 'invalid-response' }); return;
        }
        if (response.status !== 200) {
          finish({ status: 'http-error', httpStatus: response.status }); return;
        }
        if (response.headers?.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/json') {
          finish({ status: 'invalid-response' }); return;
        }
        const declared = response.headers.get('content-length');
        if (declared !== null && (!/^[0-9]+$/.test(declared) || Number(declared) > this.limits.bytes)) {
          finish({ status: 'response-limit' }); return;
        }
        if (!body || typeof body.getReader !== 'function') {
          finish({ status: 'invalid-response' }); return;
        }
        reader = body.getReader();
        const chunks = [];
        let total = 0, reads = 0;
        for (;;) {
          const part = await reader.read();
          if (done) return;
          if (part.done) break;
          if (++reads > 4096) { finish({ status: 'response-limit' }); return; }
          if (!(part.value instanceof Uint8Array)) {
            finish({ status: 'invalid-response' }); return;
          }
          total += part.value.byteLength;
          if (total > this.limits.bytes) {
            finish({ status: 'response-limit' }); return;
          }
          chunks.push(part.value);
        }
        const bytes = new Uint8Array(total);
        let offset = 0;
        for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
        let raw;
        try { raw = new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
        catch { finish({ status: 'invalid-encoding' }); return; }
        let snapshot;
        try { snapshot = parseThreadStats(raw, this.context); }
        catch { finish({ status: 'invalid-snapshot' }); return; }
        finish({ status: 'ok', snapshot });
      })().catch(() => {
        if (!done) finish({ status: controller.signal.aborted ? (timedOut ? 'timeout' : 'cancelled') : 'network-error' });
      });
    });
  }
}

function addPart(document, node, part, separator) {
  if (separator) node.append(document.createTextNode(' / '));
  if (typeof part === 'string') { node.append(document.createTextNode(part)); return; }
  const element = document.createElement(part.emphasis ? 'em' : 'span');
  element.className = part.className;
  element.dataset.tip = part.tip;
  element.title = part.tip;
  element.textContent = String(part.value);
  node.append(element);
}

export function fillThreadStatsNode(node, snapshot) {
  const document = node?.ownerDocument;
  if (!document || !snapshot) throw new TypeError('invalid-thread-stats-node');
  const parts = [];
  if (snapshot.sticky) parts.push('Sticky');
  if (snapshot.archived) parts.push('Archived');
  else if (snapshot.closed) parts.push('Closed');
  parts.push({ className: 'ts-replies', value: snapshot.replies, emphasis: snapshot.bump_limited,
    tip: snapshot.bump_limited ? 'Replies (bump limit reached)' : 'Replies' });
  parts.push({ className: 'ts-images', value: snapshot.images, emphasis: snapshot.image_limited,
    tip: snapshot.image_limited ? 'Images (limit reached)' : 'Images' });
  if (!snapshot.archived) parts.push({ className: 'ts-page', value: snapshot.page, emphasis: false, tip: 'Page' });
  node.replaceChildren();
  parts.forEach((part, index) => addPart(document, node, part, index > 0));
}

export function mountNativeThreadStats({ board, thread, settings, mobile,
  readNeverMobile = () => null,
  transport = null,
  document = globalThis.document,
  window = document?.defaultView,
  later = (action, delay) => window.setTimeout(action, delay),
  clear = handle => window.clearTimeout(handle),
} = {}) {
  if (!document || !window || typeof settings !== 'function') return null;
  let context;
  try { context = threadStatsContext({ origin: window.location.origin, board, thread }); }
  catch { return null; }
  const source = transport ?? new NativeThreadStatsTransport(context);
  if (typeof source.load !== 'function' || typeof source.cancel !== 'function') return null;
  mobile ??= window.matchMedia?.('(max-width: 480px)');

  const top = document.createElement('div');
  top.className = 'thread-stats';
  top.setAttribute('aria-label', 'Thread statistics');
  const bottom = top.cloneNode(false);
  let latest = null, timer = null, inFlight = null, queued = false;
  let suspended = false, destroyed = false, retired = false;

  function enabled() {
    if (destroyed || suspended) return false;
    let value = {};
    try { value = settings() ?? {}; } catch { return false; }
    return value.threadStats !== false && value.disableAll !== true;
  }

  function mobileLayout() {
    let preference = null;
    try { preference = readNeverMobile(); } catch { /* unavailable storage uses viewport */ }
    return mobile?.matches === true && preference !== 'true';
  }

  function removeNodes() { top.remove(); bottom.remove(); }

  function place() {
    removeNodes();
    if (!latest || !enabled()) return;
    fillThreadStatsNode(top, latest);
    if (mobileLayout()) {
      const navs = [...document.querySelectorAll('.threadNav.mobile')];
      const nav = document.querySelector('.threadNav.mobile[data-watch-position="bottom-mobile"]') ?? navs.at(-1);
      nav?.after(top);
      return;
    }
    fillThreadStatsNode(bottom, latest);
    document.querySelector('.threadNav.desktop[data-watch-position="top-desktop"]')?.append(top);
    document.querySelector('.threadNav.desktop[data-watch-position="bottom-desktop"]')?.append(bottom);
  }

  function clearTimer() {
    if (timer !== null) clear(timer);
    timer = null;
  }

  function available() {
    return enabled() && !document.hidden && window.navigator.onLine !== false && !retired && !latest?.archived;
  }

  function cancel() {
    queued = false;
    const previous = inFlight;
    inFlight = null;
    previous?.controller.abort();
    source.cancel();
    clearTimer();
  }

  function schedule() {
    clearTimer();
    if (!available() || latest?.archived) return;
    timer = later(() => { timer = null; void refresh(); }, THREAD_STATS_LIMITS.pollMs);
  }

  async function refresh() {
    if (destroyed) return { status: 'cancelled' };
    if (!enabled()) { cancel(); removeNodes(); return { status: 'disabled' }; }
    if (!available()) { cancel(); return { status: 'paused' }; }
    clearTimer();
    if (inFlight) { queued = true; return { status: 'busy' }; }
    const controller = new AbortController();
    const request = Promise.resolve().then(() => source.load({ signal: controller.signal }));
    inFlight = { request, controller };
    let result;
    try { result = await request; }
    catch { result = { status: 'network-error' }; }
    if (inFlight?.request !== request) return result;
    inFlight = null;
    const accepted = !controller.signal.aborted && available();
    controller.abort();
    if (!accepted) return result;
    if (result.status === 'ok') {
      latest = result.snapshot;
      retired = false;
      place();
    } else if (result.status === 'http-error' && result.httpStatus === 404) {
      retired = true;
    }
    const repeat = queued;
    queued = false;
    if (repeat && available() && !latest?.archived) void refresh();
    else schedule();
    return result;
  }

  function syncSettings() {
    if (!enabled()) { cancel(); removeNodes(); return; }
    place();
    void refresh();
  }
  function storage(event) {
    if (event.key === null || event.key === '4chan-settings') syncSettings();
    else if (event.key === '4chan_never_show_mobile') place();
  }
  function visibility() {
    if (document.hidden || window.navigator.onLine === false) cancel();
    else if (enabled()) void refresh();
  }
  function hide(event) {
    if (!event.persisted) { disconnect(); return; }
    suspended = true; cancel();
  }
  function show(event) {
    if (!event.persisted || destroyed) return;
    suspended = false; place();
    if (enabled()) void refresh();
  }
  function updated() { if (enabled()) void refresh(); }

  function disconnect() {
    if (destroyed) return;
    destroyed = true;
    cancel();
    inFlight?.controller.abort();
    inFlight = null;
    removeNodes();
    document.removeEventListener('4chanSettingsSaved', syncSettings);
    document.removeEventListener('boardThreadStateChanged', updated);
    document.removeEventListener('visibilitychange', visibility);
    window.removeEventListener('storage', storage);
    window.removeEventListener('online', visibility);
    window.removeEventListener('offline', visibility);
    window.removeEventListener('pagehide', hide);
    window.removeEventListener('pageshow', show);
    mobile?.removeEventListener?.('change', place);
  }

  document.addEventListener('4chanSettingsSaved', syncSettings);
  document.addEventListener('boardThreadStateChanged', updated);
  document.addEventListener('visibilitychange', visibility);
  window.addEventListener('storage', storage);
  window.addEventListener('online', visibility);
  window.addEventListener('offline', visibility);
  window.addEventListener('pagehide', hide);
  window.addEventListener('pageshow', show);
  mobile?.addEventListener?.('change', place);
  void refresh();
  return { refresh, disconnect, snapshot: () => latest };
}
