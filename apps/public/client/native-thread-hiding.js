import { NativeCatalogTransport } from './native-catalog-transport.js';
import { threadHidingKeys, readHiddenThreads, changeHiddenThread, renewHiddenThreads,
  planHiddenThreadPurge, completeHiddenThreadPurge } from './native-thread-hiding-state.js';

export function mountNativeThreadHiding({ board, threadId, settings, changed }) {
  const root = document.querySelector('.board'), keys = threadHidingKeys(board);
  if (!root || !keys) return null;
  const index = !threadId, mobile = matchMedia('(max-width: 480px)'), density = matchMedia('(min-resolution: 2dppx)');
  const worksafe = document.getElementById('watcher-context')?.dataset.worksafe !== 'false';
  const transport = new NativeCatalogTransport(), waiting = new Set(), controls = new Map(), effects = new Map();
  const explicit = new Set();
  const notice = document.createElement('p');
  notice.className = 'nativeThreadNotice'; notice.setAttribute('role', 'status'); notice.hidden = true;
  root.before(notice);
  let raw = null, purgeRaw = null, entries = new Map(), valid = true, volatile = false, retired = false;
  let ready = settings().filter !== true, filtered = new Set(), renewed = false, attempted = false, generation = 0;
  const enabled = () => !retired && settings().disableAll !== true && settings().threadHiding !== false;
  const warn = message => { notice.textContent = message; notice.hidden = !message; };
  const localWarning = () => warn('Hidden thread changes stay only in this tab. Browser storage or cross-tab locking is unavailable.');
  const sections = () => [...root.querySelectorAll('.thread')].filter(section => /^t[1-9][0-9]{0,18}$/.test(section.id));
  function load() {
    if (!volatile) {
      try { raw = localStorage.getItem(keys.hidden); purgeRaw = localStorage.getItem(keys.purge); }
      catch { volatile = true; localWarning(); }
    }
    const parsed = readHiddenThreads(raw);
    valid = parsed.status === 'ok'; entries = valid ? parsed.entries : new Map();
    if (!valid) warn('Hidden thread settings are invalid. Threads are shown; stored values were not changed.');
    return valid;
  }
  function save(next, stamp) {
    if (!volatile) {
      try {
        if (next === null) localStorage.removeItem(keys.hidden); else localStorage.setItem(keys.hidden, next);
        if (stamp !== undefined) localStorage.setItem(keys.purge, stamp);
      } catch { volatile = true; localWarning(); }
    }
    raw = next; if (stamp !== undefined) purgeRaw = stamp;
    entries = readHiddenThreads(raw).entries;
  }
  function apply() {
    for (const [section, previous] of effects) {
      section.hidden = previous.hidden;
      section.classList.remove('native-thread-hidden');
      if (!previous.stub && !filtered.has(section.id.slice(1))) section.classList.remove('post-hidden');
    }
    effects.clear();
    for (const [section, control] of controls) {
      if (!section.isConnected) { control.remove(); controls.delete(section); }
      else control.hidden = true;
    }
    if (!index || !enabled() || !valid) { changed?.(); return; }
    let family = getComputedStyle(document.documentElement).getPropertyValue('--watcher-icon-family').trim().replace(/['"]/g, '');
    if (!['futaba', 'burichan', 'tomorrow', 'photon'].includes(family)) family = 'futaba';
    for (const section of sections()) {
      const id = section.id.slice(1), suppressed = (!ready || filtered.has(id)) && !explicit.has(id);
      if (suppressed) continue;
      let control = controls.get(section);
      if (!control) {
        control = document.createElement('button'); control.type = 'button'; control.id = `sa${id}`;
        control.dataset.cmd = 'hide'; control.dataset.id = id;
        control.dataset.worksafe = String(worksafe);
        control.addEventListener('click', () => { void toggle(id); });
        controls.set(section, control);
      }
      const hidden = entries.has(id);
      control.className = mobile.matches ? 'mobileHideButton button mobile-tu-show nativeThreadRestore' : 'nativeThreadToggle';
      control.setAttribute('aria-label', mobile.matches ? 'Show Hidden Thread' : `${hidden ? 'Show' : 'Hide'} thread ${id}`);
      control.setAttribute('aria-controls', section.id);
      if (hidden) control.dataset.hidden = id; else delete control.dataset.hidden;
      if (mobile.matches) {
        control.textContent = 'Show Hidden Thread'; section.before(control); control.hidden = !hidden;
      } else {
        let image = control.querySelector('img');
        if (!image) { image = document.createElement('img'); control.replaceChildren(image); }
        image.alt = 'H'; image.className = 'extButton threadHideButton'; image.width = image.height = 18;
        image.src = `/static/watcher/${family}/post_expand_${hidden ? 'plus' : 'minus'}${density.matches ? '@2x' : ''}.png`;
        image.title = hidden ? 'Show thread' : 'Hide thread';
        section.querySelector('.opContainer')?.prepend(control); control.hidden = false;
      }
      if (hidden) {
        effects.set(section, { hidden: section.hidden, stub: section.classList.contains('post-hidden') });
        if (mobile.matches || (settings().hideStubs === true && section.dataset.sticky !== 'true')) section.hidden = true;
        else section.classList.add('post-hidden', 'native-thread-hidden');
      }
    }
    changed?.();
  }
  async function mutate(action, { allowDisabled = false, render = true } = {}) {
    if (retired || (!allowDisabled && !enabled()) || waiting.size >= 16) return false;
    const controller = new AbortController(); waiting.add(controller);
    const timer = setTimeout(() => controller.abort(), 5000);
    const run = () => {
      if (controller.signal.aborted || retired || (!allowDisabled && !enabled()) || !load()) return false;
      const result = action(); if (render) apply(); return result;
    };
    try {
      if (!navigator.locks?.request) { volatile = true; localWarning(); return run(); }
      return await navigator.locks.request(`paperboard-thread-hiding-${board}`, { signal: controller.signal }, run);
    } catch {
      if (!retired) warn('Hidden thread changes could not acquire the browser lock. Try again.');
      return false;
    } finally { clearTimeout(timer); waiting.delete(controller); }
  }
  async function toggle(id) {
    if (!index || !enabled() || !load() || !sections().some(section => section.id === `t${id}`)) return false;
    const hidden = !entries.has(id);
    const result = await mutate(() => {
      const next = changeHiddenThread(raw, id, hidden);
      if (next.status !== 'ok') { warn('Too many hidden threads, or invalid hidden thread settings.'); return false; }
      explicit.add(id); save(next.raw); return true;
    });
    if (result && mobile.matches) {
      const section = document.getElementById(`t${id}`);
      if (hidden) controls.get(section)?.focus();
      else section?.querySelector('[data-post-menu]')?.focus();
    }
    return result;
  }
  async function purge() {
    if (attempted || !enabled() || !valid || !index) return;
    attempted = true;
    const plan = planHiddenThreadPurge(board, raw, purgeRaw);
    if (plan.status === 'invalid') { warn('Hidden thread cleanup settings are invalid; saved hides were not changed.'); return; }
    if (plan.status !== 'due') return;
    const expected = generation;
    const cycle = await transport.refresh([board]);
    if (generation !== expected || !enabled()) return;
    await mutate(() => {
      if (generation !== expected) return false;
      const next = completeHiddenThreadPurge(plan, cycle, raw, purgeRaw);
      if (next.status !== 'ready') return false;
      save(next.raw, next.purgeRaw); return true;
    });
  }
  async function initialize() {
    if (renewed || !index || !ready || !enabled() || !valid) return;
    renewed = true;
    const visible = new Set(sections().map(section => section.id.slice(1)).filter(id => !filtered.has(id)));
    await mutate(() => {
      const next = renewHiddenThreads(raw, visible);
      if (next.status !== 'ok') return false;
      if (next.raw !== raw) save(next.raw);
      return true;
    });
    await purge();
  }
  async function clearHistory() {
    if (retired || !load()) return false;
    const count = entries.size;
    if (!count) { alert(`You don't have any hidden threads on /${board}/`); return false; }
    if (!confirm(`This will unhide ${count} thread${count > 1 ? 's' : ''} on /${board}/`)) return false;
    const expected = raw;
    generation++; transport.cancel();
    return mutate(() => {
      if (raw !== expected) { warn('Hidden threads changed in another tab. Open Clear History again.'); return false; }
      save(null); explicit.clear(); return true;
    }, { allowDisabled: true, render: false });
  }
  function refresh() {
    load();
    if (settings().filter !== true) { ready = true; filtered.clear(); }
    if (!enabled()) { generation++; transport.cancel(); for (const controller of waiting) controller.abort(); }
    apply(); void initialize();
  }
  function cancel() { generation++; transport.cancel(); for (const controller of waiting) controller.abort(); }
  window.addEventListener('storage', event => {
    if (event.key === null || [keys.hidden, keys.purge, '4chan-settings'].includes(event.key)) {
      cancel(); explicit.clear(); refresh();
    }
  });
  window.addEventListener('pagehide', () => { retired = true; cancel(); });
  document.addEventListener('4chanSettingsSaved', refresh);
  mobile.addEventListener('change', apply); density.addEventListener('change', apply);
  queueMicrotask(refresh);
  return { toggle, clearHistory, refresh, enabled,
    isHidden: id => entries.has(id),
    setFiltered: ids => { ready = ids !== null; filtered = ids ?? new Set(); apply(); void initialize(); },
  };
}
