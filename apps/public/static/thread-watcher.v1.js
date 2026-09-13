import { WATCH_LIMITS, postId, watchKey, splitWatchKey, watchLabel, readWatches, writeWatches,
  sameEntry, orderedWatches, autoRefreshEligible, acknowledgedEntry,
  WatcherRefresh } from './thread-watcher-core.v1.js';
import { PostTracking } from './post-tracking.v1.js';
import { installSettings } from './native-settings.v1.js';

const context = document.getElementById('watcher-context');
if (context && watchKey(context.dataset.board, '1')) start(context);

function start(context) {
  const board = context.dataset.board;
  const threadId = postId(context.dataset.thread);
  const catalog = context.dataset.catalog === 'true';
  const storeKey = '4chan-watch';
  const settingsKey = '4chan-settings';
  const timestampKey = '4chan-tw-timestamp';
  const lockName = 'paperboard-thread-watcher';
  let persistent = !!navigator.locks?.request;
  let settingsCache = {};
  let volatileSettings = false;
  let entries = readWatches(read(storeKey));
  let enabled = configuration().threadWatcher === true && configuration().disableAll !== true;
  let busy = false;
  const mobile = matchMedia('(max-width: 480px)');
  let collapsed = mobile.matches;

  function read(key) {
    try { return localStorage.getItem(key); }
    catch { persistent = false; return null; }
  }
  function configuration() {
    if (volatileSettings) return { ...settingsCache };
    let raw;
    try { raw = localStorage.getItem(settingsKey); }
    catch { persistent = false; volatileSettings = true; return { ...settingsCache }; }
    settingsCache = {};
    if (raw && raw.length <= 4096) {
      try {
        const value = JSON.parse(raw);
        if (value && typeof value === 'object' && !Array.isArray(value)) settingsCache = value;
      } catch { /* Malformed preferences use finite defaults. */ }
    }
    return { ...settingsCache };
  }
  function load() { if (persistent) entries = readWatches(read(storeKey)); }
  function node(tag, text, className) {
    const result = document.createElement(tag);
    if (text !== undefined) result.textContent = text;
    if (className) result.className = className;
    return result;
  }
  function button(text, action, className) {
    const result = node('button', text, className);
    result.type = 'button';
    result.addEventListener('click', action);
    return result;
  }
  async function locked(action) {
    if (!persistent) return action();
    try { return await navigator.locks.request(lockName, action); }
    catch { notice.textContent = 'Watch change could not be saved. Try again.'; return false; }
  }
  async function change(action, signal) {
    return locked(() => {
      if (signal?.aborted || !enabled) return false;
      load();
      const next = new Map(entries);
      if (action(next) === false) return false;
      const raw = writeWatches(next);
      if (persistent) {
        try { localStorage.setItem(storeKey, raw); }
        catch { persistent = false; }
      }
      entries = next;
      render();
      return true;
    });
  }

  const panel = node('aside', undefined, 'watcherPanel');
  panel.id = 'threadWatcher';
  panel.setAttribute('aria-label', 'Thread Watcher');
  const heading = node('div', undefined, 'watcherHeader');
  heading.id = 'twHeader';
  const fold = button('Thread Watcher', () => { collapsed = !collapsed; render(); }, 'watcherFold');
  fold.setAttribute('aria-controls', 'watcher-body');
  const refreshButton = button('Refresh', () => refreshAll(false));
  refreshButton.id = 'twPrune';
  const close = button('Close', () => { refresh.cancel(); collapsed = true; render(); });
  close.id = 'twClose';
  heading.append(fold, refreshButton, close);
  const body = node('div');
  body.id = 'watcher-body';
  const list = node('ul');
  list.id = 'watchList';
  const notice = node('p', '', 'watcherNotice');
  notice.setAttribute('role', 'status');
  body.append(list, notice);
  panel.append(heading, body);
  document.body.append(panel);

  const refresh = new WatcherRefresh({ origin: location.origin, getEntries: () => entries,
    getTracked: key => tracking.tracked(key),
    commit: (key, expected, next, signal) => change(rows => {
      if (!sameEntry(rows.get(key), expected)) return false;
      if (next) rows.set(key, next); else rows.delete(key);
    }, signal),
  });
  const tracking = new PostTracking({ board, settings: configuration, locked,
    onPost: receipt => change(rows => {
      const key = watchKey(board, receipt.thread);
      let current = rows.get(key);
      if (!current && receipt.watch && configuration().threadAutoWatcher === true) {
        if (rows.size >= WATCH_LIMITS.entries) return false;
        const section = sections().find(section => sectionId(section) === receipt.thread);
        current = { label: section ? label(section) : watchLabel('', '', receipt.thread),
          read: receipt.post, unread: 0, archived: false, ownReply: false };
      }
      if (!current) return false;
      rows.set(key, receipt.track ? acknowledgedEntry(current, receipt.post) : current);
    }),
  });

  const settingsNavigation = installSettings({ catalog, read: configuration, save: saveSettings,
    toggleWatcher: () => { collapsed = !collapsed; render(); if (!collapsed) void refreshAll(true); },
  });
  async function saveSettings(changes) {
    const applied = await locked(() => {
      const settings = { ...configuration(), ...changes };
      if (catalog && changes.threadWatcher === true) settings.disableAll = false;
      const raw = JSON.stringify(settings);
      if (raw.length > 4096) return false;
      if (persistent) {
        try { localStorage.setItem(settingsKey, raw); }
        catch { persistent = false; volatileSettings = true; }
      } else volatileSettings = true;
      settingsCache = settings;
      refresh.cancel();
      enabled = settings.threadWatcher === true && settings.disableAll !== true;
      collapsed = mobile.matches;
      render();
      return true;
    });
    if (applied === false) return false;
    if (enabled) { await acknowledgeCurrent(); navigateReadPosition(); }
    return { persisted: !volatileSettings };
  }

  function textWithBreaks(element) {
    if (!element) return '';
    let text = '';
    for (const child of element.childNodes) {
      if (child.nodeType === Node.TEXT_NODE) text += child.textContent;
      else if (child.nodeName === 'BR') text += ' ';
      else text += textWithBreaks(child);
    }
    return text;
  }
  function sections() {
    return [...document.querySelectorAll('.board > .thread, #threads > .thread'),
      ...document.getElementById('catalogFiltered')?.content.querySelectorAll('.thread') || []];
  }
  function sectionId(section) { return postId(section.dataset.threadId || section.id.replace(/^t/, '')); }
  function posts(section) {
    return [...section.querySelectorAll('.post[id]')].map(post => postId(post.id.slice(1))).filter(Boolean);
  }
  function latest(section) {
    return catalog ? postId(section.dataset.latestReply) || sectionId(section)
      : posts(section).at(-1) || sectionId(section);
  }
  function label(section) {
    const teaser = section.querySelector('.teaser') || section.querySelector('template.catalogTeaser')?.content.querySelector('.teaser');
    const subject = catalog ? teaser?.querySelector('b')?.textContent : section.querySelector('.op .subject')?.textContent;
    return watchLabel(subject, textWithBreaks(catalog ? teaser : section.querySelector('.op .postMessage')), sectionId(section));
  }
  async function toggleThread(section) {
    const id = sectionId(section);
    const key = watchKey(board, id);
    if (!key) return;
    refresh.cancel();
    await change(rows => {
      if (rows.has(key)) rows.delete(key);
      else if (rows.size < WATCH_LIMITS.entries) rows.set(key, {
        label: label(section), read: latest(section), unread: 0, archived: !!document.querySelector('.archiveNotice'), ownReply: false,
      });
      else { notice.textContent = `Watch limit: ${WATCH_LIMITS.entries} threads.`; return false; }
    });
  }
  function controls() {
    for (const section of sections()) {
      const id = sectionId(section);
      if (!id) continue;
      let control = section.querySelector('.wbtn');
      if (!control) {
        control = button('Watch', () => toggleThread(section), 'wbtn');
        control.id = `leaf-${id}`;
        (section.querySelector('.meta') || section.querySelector('.op .postInfo'))?.append(' ', control);
      }
      const watched = entries.has(watchKey(board, id));
      control.hidden = !enabled;
      control.textContent = watched ? 'Unwatch' : 'Watch';
      control.setAttribute('aria-label', `${watched ? 'Unwatch' : 'Watch'} thread ${id}`);
      control.setAttribute('aria-pressed', String(watched));
      if (threadId && enabled && watched) {
        for (const post of section.querySelectorAll('.post[id]')) {
          if (post.querySelector('.watcherLastRead')) continue;
          const position = postId(post.id.slice(1));
          if (!position) continue;
          const mark = button('Mark read here', async () => {
            refresh.cancel();
            await change(rows => { const current = rows.get(watchKey(board, id));
              if (!current) return false;
              rows.set(watchKey(board, id), acknowledgedEntry(current, position, false)); });
          }, 'watcherLastRead');
          post.querySelector('.postInfo')?.append(' ', mark);
        }
      }
      for (const mark of section.querySelectorAll('.watcherLastRead')) mark.hidden = !enabled || !watched;
    }
  }
  function render() {
    tracking.prepareForms();
    panel.hidden = !enabled || (mobile.matches && collapsed);
    panel.style.position = !catalog && !mobile.matches && configuration().fixedThreadWatcher === true ? 'fixed' : 'absolute';
    panel.style.left = mobile.matches ? '0px' : '10px';
    panel.style.top = mobile.matches ? `${window.scrollY + 30}px` : catalog ? '75px' : '380px';
    close.hidden = !mobile.matches;
    settingsNavigation.setWatcherEnabled(enabled, !panel.hidden);
    body.hidden = collapsed;
    fold.setAttribute('aria-expanded', String(!collapsed));
    refreshButton.disabled = busy || !enabled;
    list.replaceChildren();
    for (const [key, entry] of orderedWatches(entries)) {
      const { board: slug, id } = splitWatchKey(key);
      const row = node('li');
      row.id = `watch-${key}`;
      const dead = entry.read === '-1';
      const unread = !dead && entry.unread > 0;
      const link = node('a', `${unread ? `(${entry.unread}) ` : ''}/${slug}/ - ${entry.label}`);
      const fragment = catalog ? (BigInt(entry.read) > 0n ? `#p${entry.read}` : '') : `#lr${entry.read}`;
      link.href = `/${slug}/thread/${id}${fragment}`;
      if (dead) link.classList.add('deadlink');
      else {
        if (unread) link.classList.add('hasNewReplies');
        if (entry.archived) link.classList.add('archivelink');
        if (entry.ownReply) {
          link.classList.add('hasYouReplies');
          link.title = 'This thread has replies to your posts';
        }
      }
      const remove = button('\u00d7', async () => { refresh.cancel(); await change(rows => { rows.delete(key); }); }, 'watcherRemove');
      remove.setAttribute('aria-label', `Unwatch /${slug}/ thread ${id}`);
      row.append(remove, ' ', link);
      list.append(row);
    }
    if (!persistent) notice.textContent = 'Storage or cross-tab locking is unavailable. Changes stay in this tab.';
    controls();
  }
  async function refreshAll(automatic) {
    if (!enabled || busy || !entries.size || (automatic && document.hidden)) return;
    if (automatic && (threadId || !autoRefreshEligible(read(timestampKey), catalog))) return;
    const claimed = await locked(() => {
      load();
      const raw = read(timestampKey);
      if (raw !== null && !autoRefreshEligible(raw, false)) return false;
      try { localStorage.setItem(timestampKey, String(Date.now())); } catch { persistent = false; }
      return true;
    });
    if (!claimed || !enabled) { notice.textContent = 'Wait a minute before refreshing again.'; return; }
    busy = true;
    render();
    try {
      const result = await refresh.refresh();
      const failed = result.results.filter(row => row.status === 'failed').length;
      notice.textContent = result.status === 'cooldown' ? 'Wait a minute before refreshing again.'
        : result.status === 'cancelled' ? 'Refresh stopped.'
          : failed ? `${failed} thread refreshes failed. Saved state was retained.` : 'Refresh complete.';
    } finally { busy = false; render(); }
  }
  async function acknowledgeCurrent() {
    if (!enabled || !threadId) return;
    const section = document.getElementById(`t${threadId}`);
    if (!section) return;
    await change(rows => {
      const key = watchKey(board, threadId);
      const current = rows.get(key);
      if (!current) return false;
      rows.set(key, acknowledgedEntry(current, latest(section)));
    });
  }
  function navigateReadPosition() {
    if (!enabled || !threadId || !/^#lr[0-9]{1,19}$/.test(location.hash)) return;
    const read = postId(location.hash.slice(3), true);
    if (read === null) return;
    const section = document.getElementById(`t${threadId}`);
    if (!section) return;
    const target = posts(section).find(id => BigInt(id) > BigInt(read)) || read;
    const post = document.getElementById(`p${target}`);
    if (post) { post.classList.add('watcherReadTarget'); post.scrollIntoView({ block: 'nearest' }); }
    history.replaceState(null, '', location.pathname + location.search);
  }
  window.addEventListener('storage', event => {
    if (event.key !== null && ![storeKey, settingsKey].includes(event.key)) return;
    refresh.cancel();
    load();
    const settings = configuration();
    enabled = settings.threadWatcher === true && settings.disableAll !== true;
    render();
  });
  window.addEventListener('pagehide', () => refresh.cancel());
  mobile.addEventListener('change', () => { collapsed = mobile.matches; render(); });
  const container = document.getElementById('threads');
  if (container) new MutationObserver(controls).observe(container, { childList: true });
  render();
  tracking.consume(threadId).then(() => acknowledgeCurrent()).then(() => {
    navigateReadPosition(); return refreshAll(true);
  });
}
