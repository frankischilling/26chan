import { createCommentProjection } from './native-comment-projection.js';
import { geometrySize } from './math/schema.mjs';

export const MATH_LIMITS = Object.freeze({ queue: 256, deadline: 1500, restarts: 3,
  cache: 64, cacheBytes: 2097152, expressions: 256, nodes: 32768, bytes: 4194304,
  messages: 512, scanNodes: 65536 });

// Delimiters are literal and case sensitive, like the source tex2jax config.
export function mathSpans(value) {
  if (typeof value !== 'string' || value.length > 65536) return [];
  const spans = [];
  for (const match of value.matchAll(/\[(math|eqn)\]([\s\S]*?)\[\/\1\]/g)) {
    if (spans.length === MATH_LIMITS.expressions) return [];
    spans.push({ start: match.index, end: match.index + match[0].length, tex: match[2], display: match[1] === 'eqn' });
  }
  return spans;
}

export function mountNativeMath({ root, projection, board } = {}) {
  const document = root?.ownerDocument, window = document?.defaultView;
  if (!root || !projection || document.body?.dataset.mathTags !== '1' || root.closest('.catalog')) return null;
  const records = new Map(), cache = new Map(), observed = new Set(), demanded = new WeakSet(), quoted = new WeakSet();
  let queue = [], active = null, worker = null, serial = 0, timer = null, cacheBytes = 0;
  let failures = 0, disposed = false, suspended = false, scheduled = false;
  let liveExpressions = 0, liveNodes = 0, liveBytes = 0, preview = null, previewTimer = null;
  const eligible = message => message?.isConnected && (message === preview?.output
    || quoted.has(message) || (root.contains(message) && !projection.within(message)));
  function candidate(message) {
    if (!eligible(message)) return false;
    try { return /\[(?:math|eqn)\]/.test(projection.text(message)); }
    catch { return false; }
  }
  function registerQuote(popup, context) {
    // The source applies the current page's math policy to parsed quote copies.
    // Only the quote controller can register a validated popup here.
    if (!context || typeof context.board !== 'string') return;
    for (const message of popup.querySelectorAll('.postMessage')) quoted.add(message);
    schedule();
  }
  function snapshot(message) {
    const html = projection.html(message), nodes = [];
    function visit(parent) { for (const node of parent.childNodes) {
      if (projection.has(node)) continue;
      nodes.push(node); if (node.nodeType === 1) visit(node);
    } }
    visit(message); return { html, nodes };
  }
  function current(record) {
    if (disposed || suspended || records.get(record.message) !== record || !eligible(record.message)) return false;
    try {
      const next = snapshot(record.message);
      return next.html === record.snapshot.html && next.nodes.length === record.snapshot.nodes.length
        && next.nodes.every((node, index) => node === record.snapshot.nodes[index]);
    } catch { return false; }
  }
  function release(record) {
    records.delete(record.message);
    for (const run of record.runs) {
      run.output?.remove();
      for (const source of run.sources) {
        if (source.node.nodeType === 3 && source.release) {
          if (source.node.data === '') source.node.data = source.value;
          source.release();
        } else if (source.release) {
          if (source.node.getAttribute('hidden') === '') source.node.removeAttribute('hidden');
          source.release();
        }
      }
    }
    liveExpressions -= record.expressions; liveNodes -= record.nodes; liveBytes -= record.bytes;
    queue = queue.filter(job => job.record !== record);
  }
  function geometryNode(geometry, display) {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', geometry.viewBox.join(' '));
    svg.setAttribute('width', `${geometry.width}ex`); svg.setAttribute('height', `${geometry.height}ex`);
    svg.setAttribute('overflow', 'hidden');
    svg.setAttribute('fill', 'currentColor'); svg.setAttribute('focusable', 'false'); svg.setAttribute('aria-hidden', 'true');
    function append(parent, children) { for (const child of children) {
      const node = document.createElementNS('http://www.w3.org/2000/svg', child.tag);
      for (const [name, value] of Object.entries(child.attrs)) node.setAttribute(name, String(value));
      append(node, child.children ?? []); parent.append(node);
    } }
    append(svg, geometry.children);
    const wrapper = document.createElement('span'); wrapper.className = display ? 'nativeMath displayMath' : 'nativeMath';
    wrapper.setAttribute('role', 'math');
    wrapper.append(svg); return wrapper;
  }
  function commit(record) {
    if (record.remaining || !current(record)) return;
    for (const run of record.runs) {
      if (!run.spans.some(span => span.geometry)) continue;
      const output = document.createElement('span'); output.className = 'nativeMathRun';
      let offset = 0, literalNodes = 0;
      for (const span of run.spans) {
        literalNodes += appendLiteral(output, run, offset, span.start);
        const size = span.geometry && geometrySize(span.geometry);
        // Charge every live clone, not just unique cache entries. Text and UI
        // wrappers also count; failures retain the original literal input.
        if (size && liveExpressions + 1 <= MATH_LIMITS.expressions
          && liveNodes + size.nodes + 4 <= MATH_LIMITS.nodes
          && liveBytes + size.bytes + span.tex.length * 2 + 256 <= MATH_LIMITS.bytes) {
          const node = geometryNode(span.geometry, span.display); node.setAttribute('aria-label', span.tex);
          output.append(node); liveExpressions++; record.expressions++;
          const nodes = size.nodes + 4, bytes = size.bytes + span.tex.length * 2 + 256;
          liveNodes += nodes; record.nodes += nodes; liveBytes += bytes; record.bytes += bytes;
        } else literalNodes += appendLiteral(output, run, span.start, span.end);
        offset = span.end;
      }
      literalNodes += appendLiteral(output, run, offset, run.value.length);
      const overhead = run.value.length * 2 + 128, nodes = literalNodes + 1;
      if (liveBytes + overhead > MATH_LIMITS.bytes || liveNodes + nodes > MATH_LIMITS.nodes) {
        // Roll back the entire message rather than create uncharged output.
        release(record); return;
      }
      record.bytes += overhead; liveBytes += overhead; record.nodes += nodes; liveNodes += nodes;
      projection.claim(output, record); run.output = output;
      run.sources[0].node.before(output);
      for (const source of run.sources) {
        if (source.node.nodeType === 3) {
          source.release = projection.trackText(source.node, () => source.node.data === '' ? source.value : source.node.data);
          source.node.data = '';
        } else {
          source.release = projection.trackAttributes(source.node, (name, value) => name === 'hidden' && value === '' ? null : value);
          source.node.hidden = true;
        }
      }
    }
  }
  function appendLiteral(parent, run, start, end) {
    let offset = 0, nodes = 0;
    for (const source of run.sources) {
      const next = offset + source.value.length;
      if (source.node.nodeType === 3 && end > offset && start < next) {
        const text = source.value.slice(Math.max(0, start - offset), Math.min(source.value.length, end - offset));
        if (text) { parent.append(document.createTextNode(text)); nodes++; }
      } else if (source.node.nodeName === 'BR' && start <= offset && offset < end) {
        parent.append(document.createElement('br')); nodes++;
      }
      // The reference removes WBR nodes when typesetting a math-containing post.
      // BR still represents a visible line break outside a rendered equation.
      offset = next;
      if (offset >= end) break;
    }
    return nodes;
  }
  function settle(job, geometry) {
    if (!current(job.record)) return;
    job.span.geometry = geometry; job.record.remaining--;
    commit(job.record);
  }
  function stopWorker() { clearTimeout(timer); timer = null; worker?.terminate(); worker = null; }
  function catastrophic() {
    const job = active; active = null; stopWorker(); failures++;
    if (job) settle(job, null);
    if (failures > MATH_LIMITS.restarts) {
      for (const pending of queue.splice(0)) settle(pending, null);
    } else pump();
  }
  function remember(key, geometry) {
    const size = geometrySize(geometry), bytes = (size?.bytes ?? 0) + key.length * 2;
    if (!size || bytes > MATH_LIMITS.cacheBytes) return;
    while (cache.size >= MATH_LIMITS.cache || cacheBytes + bytes > MATH_LIMITS.cacheBytes) {
      const first = cache.keys().next().value; cacheBytes -= cache.get(first).bytes; cache.delete(first);
    }
    cache.set(key, { geometry, bytes }); cacheBytes += bytes;
  }
  function pump() {
    if (active || disposed || suspended || failures > MATH_LIMITS.restarts) return;
    let job;
    while ((job = queue.shift()) && !current(job.record)) { /* Drop stale demand. */ }
    if (!job) return;
    const cached = cache.get(job.key);
    if (cached) { settle(job, cached.geometry); pump(); return; }
    active = job;
    try {
      if (!worker) {
        worker = new window.Worker('/static/native-math-worker.v1.js');
        const instance = worker;
        worker.onerror = () => { if (worker === instance) catastrophic(); };
        worker.onmessageerror = () => { if (worker === instance) catastrophic(); };
        worker.onmessage = event => {
          if (worker !== instance) return;
          const result = event.data, pending = active;
          if (!pending || !result || result.id !== pending.id || typeof result.ok !== 'boolean'
            || (result.ok && !geometrySize(result.geometry))) { catastrophic(); return; }
          clearTimeout(timer); timer = null; active = null;
          if (result.ok) remember(pending.key, result.geometry);
          settle(pending, result.ok ? result.geometry : null); pump();
        };
      }
      timer = window.setTimeout(catastrophic, MATH_LIMITS.deadline);
      worker.postMessage({ id: job.id, tex: job.span.tex, display: job.span.display });
    } catch { catastrophic(); }
  }
  function inspect(message) {
    if (!eligible(message)) return;
    const previous = records.get(message);
    if (previous && current(previous)) return;
    if (previous) release(previous);
    if (records.size >= MATH_LIMITS.messages) return;
    let original; try { original = snapshot(message); } catch { return; }
    const record = { message, snapshot: original, runs: [], remaining: 0, expressions: 0, nodes: 0, bytes: 0 };
    function visit(parent) {
      let run = null;
      for (const node of parent.childNodes) {
        if (projection.has(node)) { run = null; continue; }
        if (node.nodeType === 3 || node.nodeName === 'WBR' || node.nodeName === 'BR') {
          if (!run) { run = { value: '', sources: [], spans: [] }; record.runs.push(run); }
          const value = node.nodeType === 3 ? projection.sourceText(node) : node.nodeName === 'BR' ? '\n' : '';
          run.sources.push({ node, value }); run.value += value;
        } else { run = null; if (node.nodeName !== 'PRE') visit(node); }
      }
    }
    visit(message);
    record.runs = record.runs.filter(run => (run.spans = mathSpans(run.value)).length);
    if (!record.runs.length) return;
    const count = record.runs.reduce((sum, run) => sum + run.spans.length, 0);
    if (count > MATH_LIMITS.expressions || queue.length + count > MATH_LIMITS.queue) return;
    records.set(message, record); record.remaining = count;
    for (const run of record.runs) for (const span of run.spans) {
      queue.push({ id: ++serial, record, span, key: `${span.display ? 1 : 0}:${span.tex}` });
    }
    pump();
  }
  const intersection = typeof window.IntersectionObserver === 'function' ? new window.IntersectionObserver(entries => {
    for (const entry of entries) if (entry.isIntersecting) {
      observed.delete(entry.target); demanded.add(entry.target); intersection.unobserve(entry.target); inspect(entry.target);
    }
  }, { rootMargin: '200px' }) : null;
  function refresh() {
    if (disposed || suspended) return;
    for (const record of [...records.values()]) if (!current(record)) release(record);
    // Never retain an unbounded IntersectionObserver target list or allocate
    // an unbounded querySelectorAll result on externally changed pages. Excess
    // comments stay literal, and removals free admission for later demand.
    for (const message of observed) if (!candidate(message)) { intersection?.unobserve(message); observed.delete(message); }
    const messages = new Set(); let scanned = 0;
    for (const scope of [root, document.getElementById('quote-preview')]) {
      if (!scope) continue;
      const walker = document.createTreeWalker(scope, 1);
      let node;
      while (scanned < MATH_LIMITS.scanNodes && messages.size < MATH_LIMITS.messages && (node = walker.nextNode())) {
        scanned++;
        if (node.classList.contains('postMessage') && candidate(node)) messages.add(node);
      }
    }
    if (preview) messages.add(preview.output);
    for (const message of messages) {
      if (message === preview?.output || message.closest('#quote-preview') || demanded.has(message) || !intersection) inspect(message);
      else if (!observed.has(message) && observed.size < MATH_LIMITS.messages) {
        observed.add(message); intersection.observe(message);
      }
    }
    pump();
  }
  function schedule() {
    if (scheduled || suspended || disposed) return;
    scheduled = true; queueMicrotask(() => { scheduled = false; refresh(); });
  }
  const observer = new window.MutationObserver(changes => {
    if (changes.some(change => {
      // Inline quote roots belong to the shared projection too. Their
      // insertion is explicitly registered, and removal must release output.
      if (change.type === 'childList' && change.removedNodes.length) return true;
      if (projection.originalMutation(change)) return true;
      const message = (change.target.nodeType === 1 ? change.target : change.target.parentElement)?.closest('.postMessage');
      return quoted.has(message) && projection.owner(change.target) !== records.get(message);
    })) schedule();
  });
  function observe() { observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true }); }
  function closePreview() {
    clearTimeout(previewTimer); previewTimer = null;
    if (!preview) return;
    const record = records.get(preview.output); if (record) release(record);
    preview.panel.remove(); preview = null;
    if (active && !current(active.record)) { active = null; stopWorker(); pump(); }
  }
  function restoreMessage(message) {
    const record = records.get(message);
    if (!record) return;
    release(record);
    // Keep an already bounded job in flight; its record is now stale, so it
    // cannot commit. Reuse that worker instead of refetching it for decoration.
    // Source decorators run synchronously, then math replans their new DOM.
    schedule();
  }
  function openPreview() {
    if (disposed || suspended) return;
    closePreview();
    const panel = document.createElement('div'); panel.id = 'tex-preview-cnt'; panel.className = 'UIPanel';
    const content = document.createElement('div'); content.className = 'extPanel reply';
    const header = document.createElement('div'); header.className = 'panelHeader'; header.textContent = 'TeX Preview';
    const close = document.createElement('button'); close.type = 'button'; close.textContent = '×'; close.setAttribute('aria-label', 'Close TeX preview');
    close.addEventListener('click', closePreview); header.append(close);
    const tip = document.createElement('div'); tip.id = 'tex-protip'; tip.textContent = 'Use [math][/math] tags for inline, and [eqn][/eqn] tags for block equations.';
    const input = document.createElement('textarea'); input.id = 'input-tex-preview'; input.setAttribute('aria-label', 'TeX preview input');
    const output = document.createElement('div'); output.id = 'output-tex-preview';
    content.append(header, tip, input, output); panel.append(content); document.body.append(panel);
    preview = { panel, input, output };
    input.addEventListener('input', () => {
      clearTimeout(previewTimer);
      // Invalidate pending output immediately; debounce only new work.
      const record = records.get(output); if (record) release(record);
      if (active?.record.message === output) { active = null; stopWorker(); }
      const value = input.value;
      previewTimer = window.setTimeout(() => {
        if (preview?.input !== input || input.value !== value) return;
        output.textContent = value.slice(0, 65536); inspect(output);
      }, 50);
    });
    input.focus();
  }
  function hide() {
    suspended = true; closePreview(); observer.disconnect(); intersection?.disconnect(); observed.clear();
    active = null; queue = []; stopWorker();
    for (const record of [...records.values()]) release(record);
  }
  function show() { if (!disposed && suspended) { suspended = false; failures = 0; observe(); refresh(); } }
  function disconnect() {
    hide(); disposed = true; cache.clear(); cacheBytes = 0;
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
  }
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
  observe(); refresh();
  return { refresh, disconnect, openPreview, closePreview, registerQuote, restoreMessage };
}

let pageInstance;
export function pageNativeMath() {
  if (pageInstance) return pageInstance;
  if (typeof document === 'undefined' || document.body?.dataset.mathTags !== '1') return null;
  const root = document.querySelector('.board');
  if (!root || root.closest('.catalog')) return null;
  const projection = createCommentProjection();
  const board = document.getElementById('watcher-context')?.dataset.board ?? document.body.dataset.board;
  const controller = mountNativeMath({ root, projection, board });
  return pageInstance = { projection, controller };
}
