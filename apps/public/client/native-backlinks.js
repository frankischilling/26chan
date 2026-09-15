import { postId } from '../static/thread-watcher-core.v1.js';
import { FILTER_LIMITS } from './native-filter-limits.js';

export const BACKLINK_LIMITS = Object.freeze({ posts: 20001, linksPerPost: 512,
  links: 16384, edges: 4096, nodes: 16384, depth: 32,
  html: FILTER_LIMITS.html, text: FILTER_LIMITS.field,
  previewRows: 128, previewNodes: 1024, previewChars: 32768 });

const tags = new Set(['SPAN', 'S', 'PRE', 'BR', 'WBR', 'A']);
const id = value => typeof value === 'string' && !/\D/.test(value) && postId(value) === value;
const escapeText = text => text.replace(/[&<>\u00a0]/g,
  ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '\u00a0': '&nbsp;' })[ch]);
const escapeAttribute = text => escapeText(text).replace(/"/g, '&quot;');

// This page controller never imports the HTML parser or requests another post.
// Target identity is captured when a source first enters the parser batch.
export function mountNativeBacklinks({ root, board, thread = null,
  origin = globalThis.location?.origin, settings, mobile, readNeverMobile = () => null,
  quoteTarget, changed, projection } = {}) {
  if (!root || typeof settings !== 'function' || typeof quoteTarget !== 'function'
    || typeof board !== 'string' || !/^[a-z0-9]{1,10}$/.test(board) || /[^a-z0-9]/.test(board)
    || (thread !== null && !id(thread))) return null;
  try {
    const url = new URL(origin);
    if (!['http:', 'https:'].includes(url.protocol) || url.origin !== origin) return null;
  } catch { return null; }
  if (!root.matches('.board') || root.closest('.catalog')) return null;
  const document = root.ownerDocument, window = document.defaultView;
  const page = { origin, board, thread };
  const history = new WeakMap(), records = new Map(), labels = new WeakMap();
  const suffixNodes = new WeakMap(), containers = new Map(), rowLinks = new WeakMap();
  const previews = new Map(), inlineCopies = new Map();
  let nextOrder = 0;
  let linksUsed = 0, edgesUsed = 0, disposed = false, suspended = false, scheduled = false, refreshing = false;

  function enabled() {
    if (disposed || suspended) return false;
    let config = {};
    try { config = settings() ?? {}; } catch { /* Defaults survive unavailable storage. */ }
    return config.backlinks !== false && config.disableAll !== true;
  }
  function mobileLayout() {
    let never = null;
    try { never = readNeverMobile(); } catch { /* Only the literal stored string opts out. */ }
    return mobile?.matches === true && never !== 'true';
  }
  function themeFamily() {
    const family = window.getComputedStyle(document.documentElement).getPropertyValue('--watcher-icon-family').trim();
    return ['futaba', 'burichan', 'tomorrow', 'photon'].includes(family) ? family : 'futaba';
  }
  function ownedSuffix(node) {
    const label = suffixNodes.get(node);
    return label && label.node === node && node.parentNode === label.link && node.data === label.suffix;
  }

  // Serialize only the finite message grammar, retaining attribute order and
  // HTML entity spelling. The result is filter input, never an HTML insertion.
  // Optional suffixes describe the entire final message before any mutation.
  function project(message, additions = null) {
    let nodes = 0, htmlSize = 0, textSize = 0;
    const parts = [], anchors = [];
    const emit = text => {
      htmlSize += text.length;
      if (htmlSize > BACKLINK_LIMITS.html) throw new RangeError('backlink-html');
      parts.push(text);
    };
    const text = value => {
      textSize += value.length;
      if (textSize > BACKLINK_LIMITS.text) throw new RangeError('backlink-text');
      emit(escapeText(value));
    };
    function visit(node, depth) {
      if (ownedSuffix(node) || projection?.has(node)) return;
      if (++nodes > BACKLINK_LIMITS.nodes || depth > BACKLINK_LIMITS.depth) throw new RangeError('backlink-nodes');
      if (node.nodeType === 3) { text(node.data); return; }
      if (node.nodeType !== 1 || node.namespaceURI !== 'http://www.w3.org/1999/xhtml'
        || !tags.has(node.tagName) || node.attributes.length > 64) throw new TypeError('backlink-node');
      const tag = node.localName, leaf = tag === 'br' || tag === 'wbr';
      emit(`<${tag}`);
      for (const { name, value } of projection?.attributes(node) ?? node.attributes) {
        if (value.length > BACKLINK_LIMITS.html || name.length > 128) throw new RangeError('backlink-attribute');
        emit(` ${name}="${escapeAttribute(value)}"`);
      }
      emit('>');
      if (tag === 'br' && ++textSize > BACKLINK_LIMITS.text) throw new RangeError('backlink-text');
      if (tag === 'a' && node.classList.contains('quotelink')) {
        if (anchors.length >= BACKLINK_LIMITS.linksPerPost) throw new RangeError('backlink-links');
        anchors.push(node);
      }
      if (Array.from(node.childNodes).filter(child => !projection?.has(child)).length + nodes > BACKLINK_LIMITS.nodes) throw new RangeError('backlink-nodes');
      for (const child of node.childNodes) visit(child, depth + 1);
      const suffix = additions?.get(node);
      if (suffix) {
        if (++nodes > BACKLINK_LIMITS.nodes || depth + 1 > BACKLINK_LIMITS.depth) throw new RangeError('backlink-nodes');
        text(suffix);
      }
      if (!leaf) emit(`</${tag}>`);
    }
    if (Array.from(message.childNodes).filter(child => !projection?.has(child)).length > BACKLINK_LIMITS.nodes) throw new RangeError('backlink-nodes');
    for (const child of message.childNodes) visit(child, 1);
    return { html: parts.join(''), text: textSize, nodes, anchors };
  }

  function removeLabel(label) {
    if (!label.node || !ownedSuffix(label.node)) return false;
    label.node.remove(); label.node = null; return true;
  }
  function readLabel(link) {
    const label = labels.get(link), value = link.textContent;
    if (!label?.node || !ownedSuffix(label.node)) return value;
    if (link.lastChild === label.node) return value.slice(0, -label.suffix.length);
    return Array.from(link.childNodes, node => node === label.node ? '' : node.textContent).join('');
  }
  function writeLabel(link, text) {
    const label = labels.get(link);
    if (label) removeLabel(label);
    link.textContent = text;
    schedule();
  }
  function commentHTML(message) {
    return project(message).html;
  }

  // Only direct, canonical posts in the board are sources or targets. Inlined
  // posts, popups and generated rows cannot enter this collection.
  function collect() {
    const current = new Map();
    let visited = 0;
    if (root.children.length > BACKLINK_LIMITS.posts) throw new RangeError('backlink-posts');
    for (const section of root.children) {
      if (!section.matches('.thread') || !section.id.startsWith('t')) continue;
      const parent = section.id.slice(1);
      if (!id(parent) || (thread !== null && parent !== thread)) continue;
      if (section.children.length + visited > BACKLINK_LIMITS.posts * 2) throw new RangeError('backlink-posts');
      for (const article of section.children) {
        if (++visited > BACKLINK_LIMITS.posts * 2) throw new RangeError('backlink-posts');
        if (projection?.within(article) || !article.matches('.postContainer') || !article.id.startsWith('pc') || article.children.length > 8) continue;
        const no = article.id.slice(2);
        if (!id(no) || BigInt(no) < BigInt(parent)) continue;
        const post = article.querySelector(':scope > .post');
        if (!post || post.id !== `p${no}` || post.children.length > 64) continue;
        const info = post.querySelector(':scope > .postInfo'), message = post.querySelector(':scope > .postMessage');
        if (info?.id !== `pi${no}` || message?.id !== `m${no}`) continue;
        if (current.has(no) || current.size >= BACKLINK_LIMITS.posts) throw new RangeError('backlink-posts');
        current.set(no, { no, thread: parent, article, post, info, message });
      }
    }
    return current;
  }
  function register(meta, current) {
    const record = { ...meta, order: nextOrder++, links: [], edges: 0, examined: 0 };
    try {
      const { anchors } = project(meta.message);
      if (linksUsed + anchors.length > BACKLINK_LIMITS.links) return record;
      const seen = new Set(), planned = [];
      for (const anchor of anchors) {
        const href = anchor.getAttribute('href'), ref = quoteTarget(href, page);
        if (!ref || ref.board !== board) continue;
        const target = current.get(ref.post);
        if (target && ref.thread !== null && ref.thread !== target.thread) continue;
        const op = ref.post === meta.thread && (ref.thread === null || ref.thread === meta.thread);
        const suffix = (op ? ' (OP)' : '') + (!target && thread && readLabel(anchor).charAt(2) !== '>' ? ' \u2192' : '');
        if (target) seen.add(ref.post);
        planned.push({ anchor, href, target: target?.post ?? null, no: ref.post, suffix });
      }
      if (edgesUsed + seen.size > BACKLINK_LIMITS.edges) return record;
      // A source that cannot carry its complete annotations contributes no
      // partial graph or labels. Its original links retain ordinary navigation.
      project(meta.message, new Map(planned.map(link => [link.anchor, link.suffix])));
      record.examined = anchors.length; record.edges = seen.size;
      for (const link of planned) {
        if (link.suffix) {
          link.label = { link: link.anchor, suffix: link.suffix, node: null };
          labels.set(link.anchor, link.label);
        }
        record.links.push(link);
      }
    } catch { /* Malformed or oversized original DOM is left unchanged. */ }
    return record;
  }
  function live(link, record) {
    return !projection?.within(link.anchor) && record.message.contains(link.anchor) && link.anchor.classList.contains('quotelink')
      && link.anchor.getAttribute('href') === link.href;
  }
  function annotations(record, active) {
    let modified = false;
    const additions = new Map();
    if (active) for (const link of record.links) if (link.label && live(link, record)) additions.set(link.anchor, link.suffix);
    try { if (additions.size) project(record.message, additions); }
    catch { additions.clear(); }
    for (const link of record.links) {
      const label = link.label;
      if (!label) continue;
      if (!additions.has(link.anchor)) { modified = removeLabel(label) || modified; continue; }
      if (label.node && ownedSuffix(label.node)) continue;
      label.node = document.createTextNode(label.suffix); suffixNodes.set(label.node, label);
      link.anchor.append(label.node); modified = true;
    }
    return modified;
  }
  function makeRow(record, target, layout) {
    const node = document.createElement('span'), link = document.createElement('a');
    const href = `/${board}/thread/${record.thread}#p${record.no}`;
    link.className = 'quotelink'; link.setAttribute('href', href); link.textContent = `>>${record.no}`;
    node.append(link, document.createTextNode(' '));
    const row = { node, link, href, source: record, target, companion: null };
    rowLinks.set(link, row); setCompanion(row, layout); return row;
  }
  function setCompanion(row, layout) {
    if (layout && !row.companion) {
      const link = document.createElement('a'); link.className = 'quoteLink';
      link.setAttribute('href', row.href); link.textContent = ' #';
      row.link.after(link); row.companion = link; return true;
    }
    if (!layout && row.companion) { row.companion.remove(); row.companion = null; return true; }
    return false;
  }
  function ownedRow(link) {
    const row = rowLinks.get(link);
    if (!enabled() || !row || link.parentNode !== row.node || row.node.className
      || link.getAttribute('href') !== row.href) return null;
    const block = row.owner?.node ?? row.copy?.block;
    if (!block?.isConnected || row.node.parentNode !== block) return null;
    return (row.owner && row.owner === containers.get(row.target))
      || (row.copy && inlineCopies.get(row.copy.popup) === row.copy) ? row : null;
  }
  function companion(link) {
    const row = ownedRow(link), node = row?.companion;
    return node && link.nextSibling === node && node.parentNode === row.node
      && node.getAttribute('href') === row.href && link.getAttribute('href') === row.href ? node : null;
  }
  function backlinkOwner(link) {
    const row = ownedRow(link), block = row?.owner?.node ?? row?.copy?.block;
    if (block?.className !== 'backlink') return null;
    return row.owner?.meta.message ?? row.copy.popup.querySelector(':scope > .postMessage');
  }
  function menuBoundary(post, info) {
    const container = containers.get(post);
    return enabled() && container?.menuBefore && container.meta.info === info
      && container.node.parentNode === info ? container.node : null;
  }
  function rows(current, active, layout) {
    let modified = false;
    const family = themeFamily();
    const wanted = new Map();
    if (active) for (const record of records.values()) {
      const seen = new Set();
      for (const link of record.links) {
        const target = current.get(link.no);
        if (!link.target || target?.post !== link.target || !live(link, record) || seen.has(link.no)) continue;
        seen.add(link.no);
        if (!wanted.has(link.target)) wanted.set(link.target, { meta: target, sources: [] });
        wanted.get(link.target).sources.push(record);
      }
    }
    for (const [post, container] of containers) if (!wanted.has(post)) {
      container.node.remove(); containers.delete(post); modified = true;
    }
    for (const [post, { meta, sources }] of wanted) {
      let container = containers.get(post);
      if (!container) {
        if (document.getElementById(`bl_${meta.no}`)) continue;
        const owner = history.get(meta.article);
        // Preserve the first row's placement when contributors disappear or
        // settings and page restoration recreate its container.
        owner.menuBefore ??= sources.every(source => source.order >= owner.order);
        const node = document.createElement('div'); node.id = `bl_${meta.no}`;
        container = { node, rows: new Map(), meta, menuBefore: owner.menuBefore };
        containers.set(post, container); modified = true;
      }
      const className = layout ? 'backlink mobile' : 'backlink';
      if (container.node.className !== className) { container.node.className = className; modified = true; }
      if (container.node.dataset.backlinkFamily !== family) { container.node.dataset.backlinkFamily = family; modified = true; }
      const parent = layout ? post : meta.info;
      if (container.node.parentNode !== parent) { parent.append(container.node); modified = true; }
      const desired = new Set(sources);
      for (const [source, row] of container.rows) if (!desired.has(source)) {
        row.node.remove(); container.rows.delete(source); modified = true;
      }
      let cursor = container.node.firstChild;
      for (const record of sources) {
        let row = container.rows.get(record);
        if (!row) {
          row = makeRow(record, post, layout); row.owner = container;
          container.rows.set(record, row); modified = true;
        } else modified = setCompanion(row, layout) || modified;
        if (row.node === cursor) cursor = cursor.nextSibling;
        else { container.node.insertBefore(row.node, cursor); modified = true; }
      }
    }
    return modified;
  }

  function removePreview(popup) {
    const entry = previews.get(popup);
    if (!entry) return;
    entry.block?.remove(); entry.dotted?.classList.remove('dotted'); previews.delete(popup);
  }
  function removeInlineCopy(popup) {
    const entry = inlineCopies.get(popup);
    if (!entry) return;
    entry.block?.remove(); entry.dotted?.classList.remove('dotted'); inlineCopies.delete(popup);
  }
  // Copy only bounded metadata from the existing graph. These links never
  // register as sources and each inline gets its own removable ownership entry.
  function prepareInlineCopy(localPost, sourceLink) {
    if (!enabled() || inlineCopies.size >= 16) return null;
    const owner = containers.get(localPost), source = ownedRow(sourceLink);
    const layout = mobileLayout(), rows = owner ? [...owner.rows.values()] : [];
    if (rows.length > BACKLINK_LIMITS.previewRows) return null;
    const sourceNo = source?.owner?.meta.no ?? source?.copy?.no;
    let nodes = rows.length ? 1 : 0, characters = rows.length ? 96 : 0;
    for (const row of rows) {
      nodes += layout ? 6 : 4;
      characters += 80 + row.href.length * (layout ? 2 : 1) + row.source.no.length;
    }
    if (sourceNo) characters += 7;
    if (nodes > BACKLINK_LIMITS.previewNodes || characters > BACKLINK_LIMITS.previewChars) return null;
    const family = themeFamily(), no = localPost.id.slice(1);
    return { nodes, characters, mount(popup) {
      const entry = { popup, block: null, dotted: null, no };
      if (rows.length) {
        const block = document.createElement('div'); entry.block = block;
        block.className = layout ? 'backlink mobile' : 'backlink'; block.dataset.backlinkFamily = family;
        for (const row of rows) {
          const copy = makeRow(row.source, null, layout); copy.copy = entry; block.append(copy.node);
        }
        const parent = layout ? popup : popup.querySelector(':scope > .postInfo');
        parent?.append(block);
      }
      if (sourceNo) {
        const quotes = [...(popup.querySelector(':scope > .postMessage')?.children ?? [])].filter(node => node.matches('a.quotelink'));
        const match = quotes.length > 1 && quotes.find(link => link.textContent === `>>${sourceNo}`);
        if (match) { match.classList.add('dotted'); entry.dotted = match; }
      }
      inlineCopies.set(popup, entry);
      return () => removeInlineCopy(popup);
    } };
  }
  function decoratePreview(popup, localPost, sourceLink, remaining = {}) {
    removePreview(popup);
    if (!enabled()) return;
    // The popup's finite post recipe has already been validated and constructed.
    // These additional rows are created only from this controller's known IDs.
    const owner = containers.get(localPost), source = rowLinks.get(sourceLink);
    let block = null, dotted = null, nodes = 0, chars = 0;
    if (owner && owner.rows.size <= BACKLINK_LIMITS.previewRows) {
      nodes = 1;
      chars = 96;
      const layout = mobileLayout();
      for (const row of owner.rows.values()) {
        nodes += layout ? 6 : 4;
        chars += 80 + row.href.length * (layout ? 2 : 1) + row.source.no.length;
      }
      if (nodes <= Math.min(BACKLINK_LIMITS.previewNodes, remaining.nodes ?? 0)
        && chars <= Math.min(BACKLINK_LIMITS.previewChars, remaining.characters ?? 0)) {
        block = document.createElement('div'); block.className = layout ? 'backlink mobile' : 'backlink';
        block.dataset.backlinkFamily = themeFamily();
        for (const row of owner.rows.values()) block.append(makeRow(row.source, null, layout).node);
        const parent = layout ? popup : popup.querySelector('.postInfo');
        if (parent) parent.append(block); else block = null;
      }
    }
    if (!block) chars = 0;
    if (source?.owner && source.owner === containers.get(source.target) && sourceLink.parentElement === source.node
      && source.owner.node.id === `bl_${source.owner.meta.no}`
      && !source.node.className && source.node.parentElement === source.owner.node) {
      const message = popup.querySelector('.postMessage');
      const quotes = message ? Array.from(message.children).filter(node => node.matches('a.quotelink')) : [];
      if (quotes.length > 1 && chars + 7 <= (remaining.characters ?? 0)) {
        const match = quotes.find(link => link.textContent === `>>${source.owner.meta.no}`);
        if (match && !match.classList.contains('dotted')) { match.classList.add('dotted'); dotted = match; }
      }
    }
    for (const previous of previews.keys()) removePreview(previous);
    if (block || dotted) previews.set(popup, { block, dotted });
  }
  function restore() {
    let modified = false;
    for (const record of records.values()) modified = annotations(record, false) || modified;
    for (const container of containers.values()) { container.node.remove(); modified = true; }
    containers.clear();
    for (const popup of previews.keys()) removePreview(popup);
    for (const popup of inlineCopies.keys()) removeInlineCopy(popup);
    if (modified) changed?.();
  }
  function refresh() {
    if (disposed || suspended || refreshing) return;
    refreshing = true;
    try {
      const current = collect(), active = enabled();
      if (!active) for (const popup of inlineCopies.keys()) removeInlineCopy(popup);
      for (const [article, record] of records) if (current.get(record.no)?.post !== record.post) {
        annotations(record, false); records.delete(article); linksUsed -= record.examined; edgesUsed -= record.edges;
      }
      for (const meta of current.values()) if (!records.has(meta.article)) {
        let record = history.get(meta.article);
        if (!record || record.post !== meta.post || record.message !== meta.message) {
          record = register(meta, current); history.set(meta.article, record);
        }
        if (linksUsed + record.examined > BACKLINK_LIMITS.links || edgesUsed + record.edges > BACKLINK_LIMITS.edges) continue;
        records.set(meta.article, record); linksUsed += record.examined; edgesUsed += record.edges;
      }
      let modified = false;
      for (const record of records.values()) modified = annotations(record, active) || modified;
      modified = rows(current, active, mobileLayout()) || modified;
      if (modified) {
        for (const popup of previews.keys()) removePreview(popup);
        changed?.();
      }
    } catch { restore(); }
    finally { refreshing = false; }
  }
  function schedule() {
    if (disposed || suspended || scheduled) return;
    scheduled = true;
    queueMicrotask(() => { scheduled = false; refresh(); });
  }
  const observer = new window.MutationObserver(schedule);
  const observe = () => observer.observe(root, { childList: true, subtree: true, characterData: true,
    attributes: true, attributeFilter: ['href', 'class', 'id'] });
  const storage = event => { if (event.key === null || ['4chan-settings', '4chan_never_show_mobile'].includes(event.key)) refresh(); };
  const hide = event => {
    suspended = true; observer.disconnect(); restore();
    if (!event.persisted) disconnect();
  };
  const show = event => { if (event.persisted && !disposed) { suspended = false; observe(); refresh(); } };
  function disconnect() {
    if (disposed) return;
    disposed = true; observer.disconnect(); restore(); records.clear();
    window.removeEventListener('storage', storage); window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
    document.removeEventListener('4chanSettingsSaved', refresh); mobile?.removeEventListener?.('change', refresh);
  }
  window.addEventListener('storage', storage); window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
  document.addEventListener('4chanSettingsSaved', refresh); mobile?.addEventListener?.('change', refresh);
  observe(); refresh();
  return { refresh, disconnect, readLabel, writeLabel, commentHTML, companion, menuBoundary, decoratePreview,
    backlinkOwner, prepareInlineCopy };
}
