import { postId } from '../static/thread-watcher-core.v1.js';

export const HIDDEN_REPLY_LIMITS = Object.freeze({ entries: 512, storage: 65536, age: 604800000 });

export function readHiddenReplies(raw, now = Date.now()) {
  if (raw === null) return { status: 'ok', entries: new Map() };
  if (typeof raw !== 'string' || raw.length > HIDDEN_REPLY_LIMITS.storage) return { status: 'invalid' };
  try {
    const value = JSON.parse(raw);
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('record');
    const entries = Object.entries(value);
    if (entries.length > HIDDEN_REPLY_LIMITS.entries || entries.some(([id, time]) => postId(id) !== id
      || !Number.isSafeInteger(time) || time < 0 || time > now)) throw new Error('entry');
    return { status: 'ok', entries: new Map(entries) };
  } catch { return { status: 'invalid' }; }
}

export function renewHiddenReplies(entries, visible, now = Date.now()) {
  const next = new Map();
  for (const [id, time] of entries) {
    if (visible.has(id)) next.set(id, now);
    else if (now - time <= HIDDEN_REPLY_LIMITS.age) next.set(id, time);
  }
  return next;
}

export function mountNativeReplyHiding({ board, settings, changed }) {
  const root = document.querySelector('.board');
  if (!root || !/^[a-z0-9]{1,10}$/.test(board)) return null;
  const key = `4chan-hide-r-${board}`;
  const lock = `paperboard-reply-hiding-${board}`;
  const notice = document.createElement('p');
  notice.className = 'nativeReplyNotice'; notice.setAttribute('role', 'status'); notice.hidden = true;
  root.before(notice);
  let entries = new Map(), volatile = false, valid = true, retired = false;
  let ready = settings().filter !== true, filtered = new Set(), renewed = false;
  const explicit = new Set();
  const effects = new Map(), waiting = new Set();
  const warn = message => { notice.textContent = message; notice.hidden = !message; };
  const localWarning = () => warn('Hidden reply changes stay only in this tab. Browser storage or cross-tab locking is unavailable.');
  const disabled = () => retired || settings().disableAll === true;
  const reply = id => {
    if (postId(id) !== id) return null;
    const element = document.getElementById(`pc${id}`);
    return element?.matches('.replyContainer') && root.contains(element) ? element : null;
  };
  function load() {
    if (volatile) return valid;
    let raw;
    try { raw = localStorage.getItem(key); }
    catch { volatile = true; localWarning(); return valid; }
    const result = readHiddenReplies(raw);
    valid = result.status === 'ok';
    entries = valid ? result.entries : new Map();
    if (!valid) warn('Hidden reply settings are invalid. Posts are shown; stored values were not changed.');
    else warn('');
    return valid;
  }
  function apply() {
    for (const [element, previous] of effects) {
      if (!previous.hidden) element.classList.remove('post-hidden');
      if (previous.arrow) {
        if (previous.marker === null) previous.arrow.removeAttribute('data-hidden');
        else previous.arrow.setAttribute('data-hidden', previous.marker);
      }
    }
    effects.clear();
    if (!disabled() && valid) {
      for (const id of entries.keys()) {
        if (!explicit.has(id) && (!ready || filtered.has(id))) continue;
        const element = reply(id);
        if (!element) continue;
        const arrow = element.querySelector('.sideArrows');
        effects.set(element, { hidden: element.classList.contains('post-hidden'), arrow,
          marker: arrow?.getAttribute('data-hidden') ?? null });
        element.classList.add('post-hidden');
        arrow?.setAttribute('data-hidden', id);
      }
    }
    changed();
  }
  function refresh() {
    if (disabled()) for (const controller of waiting) controller.abort();
    if (settings().filter !== true) { ready = true; filtered.clear(); }
    load(); apply();
    renewVisited();
  }
  function save(next, manual = null) {
    const raw = next.size ? JSON.stringify(Object.fromEntries(next)) : null;
    if (next.size > HIDDEN_REPLY_LIMITS.entries || (raw && raw.length > HIDDEN_REPLY_LIMITS.storage)) {
      warn('Hidden reply storage is full. Unhide a reply before hiding another.'); return false;
    }
    if (!volatile) {
      try { if (raw === null) localStorage.removeItem(key); else localStorage.setItem(key, raw); }
      catch { volatile = true; }
    }
    entries = next;
    if (manual) { if (manual.hide) explicit.add(manual.id); else explicit.delete(manual.id); }
    if (volatile) localWarning(); else warn('');
    apply(); return true;
  }
  async function mutate(action) {
    if (disabled() || waiting.size >= 16) return false;
    const controller = new AbortController(); waiting.add(controller);
    const timer = setTimeout(() => controller.abort(), 5000);
    let entered = false;
    const commit = () => {
      entered = true;
      if (controller.signal.aborted || disabled() || !load()) return false;
      return action();
    };
    try {
      if (!volatile && navigator.locks?.request) {
        return await navigator.locks.request(lock, { signal: controller.signal }, commit);
      }
      volatile = true; localWarning(); return commit();
    } catch {
      if (controller.signal.aborted) {
        if (!disabled()) warn('Hidden reply change timed out. Try again.');
        return false;
      }
      if (entered || disabled()) return false;
      volatile = true; localWarning(); return commit();
    } finally { clearTimeout(timer); waiting.delete(controller); }
  }
  function isHidden(id) { return !disabled() && valid && effects.has(reply(id)); }
  function toggle(id) {
    if (!reply(id) || disabled()) return Promise.resolve(false);
    // Preserve the requested action while waiting, rather than toggling a newer tab's edit.
    const hide = !isHidden(id);
    return mutate(() => {
      if (!reply(id)) return false;
      const next = renewHiddenReplies(entries, new Set());
      if (hide) next.set(id, Date.now()); else next.delete(id);
      return save(next, { id, hide });
    });
  }
  function renewVisited() {
    if (renewed || !ready || !valid || disabled()) return;
    renewed = true;
    if (!entries.size) return;
    void mutate(() => {
      if (!ready) return false;
      const visible = new Set([...root.querySelectorAll('.replyContainer[id]')]
        .map(element => element.id.slice(2)).filter(id => !filtered.has(id)));
      return save(renewHiddenReplies(entries, visible));
    });
  }
  function setFiltered(hidden) {
    ready = hidden !== null;
    if (ready) filtered = new Set(hidden);
    apply();
    renewVisited();
  }
  window.addEventListener('storage', event => {
    if (event.key === null || event.key === key || event.key === '4chan-settings') refresh();
  });
  window.addEventListener('pagehide', () => {
    retired = true;
    for (const controller of waiting) controller.abort();
  });
  new MutationObserver(changes => {
    if (changes.some(change => [...change.addedNodes, ...change.removedNodes].some(node => node.nodeType === 1
      && (node.matches('.postContainer,.thread') || node.querySelector('.postContainer'))))) refresh();
  }).observe(root, { childList: true, subtree: true });
  load(); apply();
  renewVisited();
  return { refresh, isHidden, toggle, setFiltered };
}
