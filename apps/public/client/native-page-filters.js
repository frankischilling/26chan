import { FILTER_LIMITS } from './native-filter-limits.js';
import { readFilterRules, filterColor } from './native-filter-rules.js';
import { filterEditor } from './native-filter-editor.js';

function selectedFilter() {
  try {
    const selection = window.getSelection();
    const element = selection?.anchorNode?.parentElement;
    const pattern = selection?.toString().trim() ?? '';
    const type = element?.classList.contains('name') ? 1
      : element?.classList.contains('postertrip') ? 0
        : element?.classList.contains('subject') ? 5
          : element?.matches('.posteruid,.hand') ? 4
            : element?.matches('.fileText,.file > p > a') ? 6 : 2;
    return { pattern, type };
  } catch { return { pattern: null, type: 2 }; }
}

export function mountNativeFilters({ board, threadId, settings, read, save, match, getTracked, changed }) {
  const root = document.querySelector('.board');
  const notice = document.createElement('p'); notice.className = 'nativeFilterNotice'; notice.setAttribute('role', 'status');
  root?.before(notice);
  const storageNotice = document.createElement('p');
  storageNotice.className = 'nativeFilterStorageNotice'; storageNotice.hidden = true;
  storageNotice.setAttribute('role', 'status');
  root?.before(storageNotice);
  let controller, generation = 0, signature, scheduled = false;
  const effects = new Map(), revealed = new Set();
  function clear() {
    for (const [element, prior] of effects) {
      if (!prior.hidden) element.classList.remove('post-hidden');
      if (!prior.highlight) element.classList.remove('filter-hl');
      element.classList.remove('native-filter-no-stub');
      if (element.style.boxShadow === prior.assigned) element.style.boxShadow = prior.shadow;
      prior.preview?.remove();
    }
    effects.clear();
  }
  function collect() {
    const nodes = [...root.querySelectorAll('.post[id]')];
    if (nodes.length > 20001) throw new Error('page-limit');
    const rows = [], ids = new Set();
    let size = 0;
    for (const post of nodes) {
      const id = post.id.slice(1), section = post.closest('.thread');
      const parent = section?.id.slice(1);
      if (!/^[1-9][0-9]{0,18}$/.test(id) || ids.has(id)) throw new Error('invalid-post');
      ids.add(id);
      // Native parsePost filters replies; the separate board-index path handles OPs.
      if ((threadId && id === parent) || getTracked(`${parent}-${board}`).has(id)) continue;
      const info = post.querySelector('.postInfo');
      const message = post.querySelector('.postMessage');
      if (!info || !message || !section) throw new Error('invalid-post');
      const value = { no: id, com: message.innerHTML };
      for (const [key, selector] of [['name', '.name'], ['trip', '.postertrip'], ['id', '.posteruid > :first-child'], ['sub', '.subject']]) {
        const element = info.querySelector(selector); if (element) value[key] = element.textContent;
      }
      value.filename = post.querySelector('.file > p > a')?.textContent ?? '';
      for (const [key, text] of Object.entries(value)) {
        if (text.length > (key === 'com' ? FILTER_LIMITS.html : FILTER_LIMITS.field)) throw new Error('field-limit');
      }
      size += JSON.stringify(value).length;
      if (size > 16 * 1024 * 1024) throw new Error('page-limit');
      rows.push({ id, value, post, info, section, op: id === parent });
    }
    return rows;
  }
  async function refresh() {
    controller?.abort(); controller = new AbortController();
    const signal = controller.signal, current = ++generation;
    if (!root) return;
    const config = settings(), raw = read();
    const nextSignature = JSON.stringify([raw, config.filter === true, config.hideStubs === true, config.disableAll === true]);
    if (nextSignature !== signature) { revealed.clear(); signature = nextSignature; }
    if (config.filter !== true || config.disableAll === true) { clear(); notice.textContent = ''; return; }
    notice.textContent = 'Applying filters...';
    const timeout = setTimeout(() => controller?.signal === signal && controller.abort(), 60000);
    try {
      const parsed = readFilterRules(raw);
      if (parsed.status !== 'ok') throw new Error('invalid-settings');
      const rules = parsed.rules;
      for (const rule of rules) if (filterColor(rule.color) === null) throw new Error('invalid-color');
      const rows = collect(), matches = [];
      const base = JSON.stringify({ version: 1, board, filters: rules, posts: [], mode: 'page', thread: !!threadId }).length;
      for (let offset = 0; offset < rows.length;) {
        let size = base; const batch = [];
        while (offset < rows.length && batch.length < FILTER_LIMITS.posts) {
          const row = rows[offset]; const length = JSON.stringify(row.value).length + 1;
          if (size + length > FILTER_LIMITS.request) break;
          size += length; batch.push(row.value); offset++;
        }
        if (!batch.length) throw new Error('request-limit');
        const result = await match(rules, board, batch, { mode: 'page', thread: !!threadId, signal });
        if (signal.aborted || current !== generation) return;
        if (result.status !== 'ok') throw new Error('matching-failed');
        matches.push(...result.matches);
      }
      const latest = settings();
      if (signal.aborted || current !== generation || read() !== raw || latest.filter !== true
        || latest.disableAll === true || (latest.hideStubs === true) !== (config.hideStubs === true)) return;
      clear();
      const byId = new Map(rows.map(row => [row.id, row]));
      for (const result of matches) {
        const row = byId.get(result.id), rule = rules[result.filter];
        if (!row?.post.isConnected || !rule || revealed.has(row.id)) continue;
        const element = row.op ? row.section : row.post;
        const prior = { hidden: element.classList.contains('post-hidden'), highlight: element.classList.contains('filter-hl'),
          shadow: element.style.boxShadow, assigned: element.style.boxShadow };
        effects.set(element, prior);
        if (rule.hide) {
          element.classList.add('post-hidden');
          if (row.op && config.hideStubs === true && row.section.dataset.sticky !== 'true') element.classList.add('native-filter-no-stub');
          else {
            const preview = document.createElement(row.op ? 'a' : 'button');
            if (row.op) preview.href = `/${board}/thread/${row.id}`; else preview.type = 'button';
            preview.className = 'filter-preview'; preview.dataset.cmd = 'unfilter'; preview.dataset.filtered = '1';
            preview.textContent = '[View]'; preview.setAttribute('aria-label', `View filtered ${row.op ? 'thread' : 'post'} ${row.id}`);
            preview.addEventListener('click', event => {
              if (row.op) return;
              event.preventDefault(); revealed.add(row.id); element.classList.remove('post-hidden'); preview.remove();
            });
            row.info.append(preview); prior.preview = preview;
          }
        } else {
          element.classList.add('filter-hl');
          const color = filterColor(rule.color);
          if (color) { element.style.boxShadow = `-3px 0 ${color}`; prior.assigned = element.style.boxShadow; }
        }
      }
      notice.textContent = '';
    } catch {
      if (current === generation) { clear(); notice.textContent = 'Filters could not be applied. Posts are shown.'; }
    } finally { clearTimeout(timeout); }
  }
  function schedule() {
    if (scheduled) return; scheduled = true;
    queueMicrotask(() => { scheduled = false; void refresh(); });
  }
  if (root) new MutationObserver(changes => {
    if (changes.some(change => change.target.parentElement?.closest('.postMessage,.name,.subject,.postertrip,.posteruid,.file')
      || change.target.closest?.('.postMessage,.name,.subject,.postertrip,.posteruid,.file')
      || [...change.addedNodes, ...change.removedNodes].some(node => node.nodeType === 1
        && (node.matches('.post,.postContainer,.thread') || node.querySelector('.post'))))) schedule();
  }).observe(root, { childList: true, subtree: true, characterData: true });
  window.addEventListener('pagehide', () => { generation++; controller?.abort(); });
  const open = filterEditor({ board, read, save, match, changed: result => {
    if (result.persisted === false) {
      storageNotice.textContent = 'Filters are saved only in this tab. Browser storage or cross-tab locking is unavailable.';
      storageNotice.hidden = false;
    }
    changed(); void refresh();
  } });
  return { refresh, open, selection: selectedFilter,
    addSelection: (opener, selected) => open(opener, selected) };
}
