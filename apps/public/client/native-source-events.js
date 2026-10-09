// The pinned Parser.parseThread emits the normalized start and exclusive end.
// Keep its truthy/default arithmetic, including zero limits and negative starts.
export function sourceParsingRange(length, offset, limit) {
  const start = offset ? offset < 0 ? length + offset : offset : 0;
  return { offset: start, limit: limit ? start + limit : length };
}

export function dispatchSourceEvent(document, name, detail) {
  const event = document.createEvent('Event');
  event.initEvent(name, false, false);
  if (detail) event.detail = detail;
  document.dispatchEvent(event);
}

// Source depaging receives JSON numbers; avoid lossy conversion of large IDs.
export function sourceDepagerThreadId(id) {
  const numeric = Number(id);
  return Number.isSafeInteger(numeric) ? numeric : id;
}

// Capture canonical posts only. Identity checks reject replaced async ranges.
export function captureParsingRange(section, offset, limit, threadId = section.id.slice(1)) {
  const document = section.ownerDocument;
  const posts = [...section.querySelectorAll(':scope > .postContainer')];
  const range = sourceParsingRange(posts.length, offset, limit);
  const parent = section.parentNode;
  let emitted = false;
  return {
    current: () => {
      const live = [...section.querySelectorAll(':scope > .postContainer')];
      return section.isConnected && section.parentNode === parent
        && document.getElementById(section.id) === section && posts.length === live.length
        && posts.every((post, index) => post.isConnected && post.parentNode === section
          && document.getElementById(post.id) === post && live[index] === post);
    },
    emit() {
      if (emitted || !this.current()) return false;
      emitted = true;
      dispatchSourceEvent(document, '4chanParsingDone', { threadId, ...range });
      return true;
    },
  };
}

export async function settleInitialParsing({ sections, settle, active, signal, emitted = new WeakSet() }) {
  const ranges = sections.filter(section => !emitted.has(section)).map(section => ({ section, range: captureParsingRange(section) }));
  try {
    if (signal.aborted || !active()) return false;
    if (await settle(signal) === false || signal.aborted || !active()) return false;
    if (!ranges.every(({ range }) => range.current())) return false;
    for (const { section, range } of ranges) {
      if (signal.aborted || !active()) return false;
      if (!range.emit()) return false;
      emitted.add(section);
    }
    return true;
  } catch { return false; }
}

// Dynamic controller actions remain gated until the initial transaction has
// consumed receipts, applied features and published every initial range.
export function createParsingBootstrap(settings) {
  let ready = false, cycle;
  const emitted = new WeakSet();
  return {
    ready: () => ready,
    run(options) {
      if (ready) return Promise.resolve(true);
      if (cycle && !cycle.signal.aborted) return cycle.pending;
      const current = { signal: options.signal }; cycle = current;
      current.pending = (async () => {
        try {
          if (await options.prepare(options.signal) === false || options.signal.aborted || cycle !== current) return false;
          const complete = await settleInitialParsing({ ...options, emitted });
          if (cycle !== current || options.signal.aborted) return false;
          ready = complete || settings().disableAll === true;
          return ready;
        } catch { return false; }
      })();
      return current.pending;
    },
  };
}

// Only the bootstrap owner uses this gate. Public lifecycle notifications do
// not grant permission to mount. An import may finish while the page is away.
export function createInitialMountLifecycle(window) {
  let suspended = false, retired = false;
  const waiting = new Set();
  const release = () => { for (const resolve of waiting) resolve(!retired); waiting.clear(); };
  const hide = event => {
    suspended = true;
    if (!event.persisted) { retired = true; release(); }
  };
  const show = event => {
    if (event.persisted && !retired) { suspended = false; release(); }
  };
  window.addEventListener('pagehide', hide);
  window.addEventListener('pageshow', show);
  return {
    active: () => !suspended && !retired,
    wait: () => suspended && !retired ? new Promise(resolve => waiting.add(resolve)) : Promise.resolve(!retired),
    disconnect() {
      retired = true; release();
      window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
    },
  };
}
