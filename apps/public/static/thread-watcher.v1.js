import { WATCH_LIMITS, postId, watchKey, splitWatchKey, watchLabel, readWatches, writeWatches,
  sameEntry, orderedWatches, autoRefreshEligible, acknowledgedEntry,
  WatcherRefresh } from './thread-watcher-core.v1.js';
import { PostTracking } from './post-tracking.v1.js';
import { installSettings } from './native-settings.v1.js';
import { mountWatcherPosition } from './watcher-position.v1.js';
import { NativeCatalogTransport, NativeFilterMatcher, NativeWatchLock, readNativeFilters, autoWatchBoards, mountNativeFilters, mountNativeReplyHiding, mountNativeThreadHiding, mountNativeThreadUpdater, mountNativeKeybinds, mountNativeQuickReply, markNativeTrackedQuotes,
  readBlacklist, writeBlacklist, collectAutoWatches, planAutoWatches, mountNativeLinkification, mountNativeQuotePreview, quoteTarget } from './native-filter.v1.js';
import { mountNativeBacklinks } from './native-backlinks.v1.js';

const context = document.getElementById('watcher-context');
if (context && watchKey(context.dataset.board, '1')) start(context);

function start(context) {
  const board = context.dataset.board;
  const threadId = postId(context.dataset.thread);
  const catalog = context.dataset.catalog === 'true';
  const storeKey = '4chan-watch';
  const settingsKey = '4chan-settings';
  const timestampKey = '4chan-tw-timestamp';
  const blacklistKey = '4chan-watch-bl';
  const filterKey = '4chan-filters';
  const lockName = 'paperboard-thread-watcher';
  const hasLocks = typeof navigator.locks?.request === 'function';
  let persistent = hasLocks;
  const mutationLock = new NativeWatchLock({
    acquire: hasLocks ? async (action, signal) => {
      let entered = false;
      try { return await navigator.locks.request(lockName, { signal }, () => { entered = true; return action(); }); }
      catch (error) {
        if (entered || signal.aborted || error?.name !== 'SecurityError') throw error;
        // Browser policy can expose Web Locks while denying their use. Every
        // participant must become volatile before any unlocked callback runs.
        configuration(); filterCache = read(filterKey);
        persistent = false; volatileSettings = true; volatileFilters = true;
        tracking.persistent = false;
        mutationLock.acquire = null;
        return action();
      }
    } : null,
    warn: text => { notice.textContent = text; },
  });
  let settingsCache = {};
  let volatileSettings = false;
  let volatileFilters = false;
  let filterCache = null;
  let timestampCache = null;
  let entries = readWatches(read(storeKey));
  let enabled = configuration().threadWatcher === true && configuration().disableAll !== true;
  let busy = false;
  let blacklistCache = new Set();
  let invalidBlacklist = false;
  let activePostMenu = null;
  const mobile = matchMedia('(max-width: 480px)');
  let collapsed = mobile.matches;

  function readNeverMobile() {
    try { return localStorage.getItem('4chan_never_show_mobile'); }
    catch { return null; }
  }

  function read(key) {
    if (key === filterKey && volatileFilters) return filterCache;
    if (key === timestampKey && !persistent) return timestampCache;
    try {
      const value = localStorage.getItem(key);
      if (key === timestampKey) timestampCache = value;
      return value;
    }
    catch { persistent = false; return key === timestampKey ? timestampCache : null; }
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
  function loadBlacklist() {
    if (persistent) {
      const raw = read(blacklistKey);
      if (persistent) {
        const parsed = readBlacklist(raw);
        invalidBlacklist = parsed.status !== 'ok';
        if (!invalidBlacklist) blacklistCache = parsed.keys;
      }
    }
    return invalidBlacklist ? null : new Set(blacklistCache);
  }
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
  async function locked(action, signal) {
    return mutationLock.run(action, signal);
  }
  async function change(action, signal, remember = null) {
    return locked(() => {
      const settings = configuration();
      if (signal?.aborted || !enabled || settings.threadWatcher !== true || settings.disableAll === true) return false;
      load();
      const next = new Map(entries);
      if (action(next) === false) return false;
      const raw = writeWatches(next);
      if (remember && entries.has(remember) && !next.has(remember)) {
        const blocked = loadBlacklist();
        if (!blocked) { notice.textContent = 'Watch blacklist is invalid. The watch was retained.'; return false; }
        blocked.add(remember);
        let saved;
        try { saved = writeBlacklist(blocked); }
        catch { notice.textContent = 'Watch blacklist is full. The watch was retained.'; return false; }
        // Record suppression before removing the watch. A failed second write
        // must never leave a persisted removal without its blacklist entry.
        if (persistent) {
          try { localStorage.setItem(blacklistKey, saved); }
          catch { persistent = false; }
        }
        blacklistCache = blocked;
      }
      if (persistent) {
        try { localStorage.setItem(storeKey, raw); }
        catch { persistent = false; }
      }
      entries = next;
      render();
      return true;
    }, signal);
  }

  const panel = node('aside', undefined, 'watcherPanel');
  panel.id = 'threadWatcher';
  panel.setAttribute('aria-label', 'Thread Watcher');
  const heading = node('div', undefined, 'watcherHeader');
  heading.id = 'twHeader';
  const title = node('span', 'Thread Watcher', 'watcherTitle');
  const refreshButton = button('Refresh', () => refreshAll(false));
  refreshButton.id = 'twPrune';
  const close = button('Close', () => { refresh.cancel(); collapsed = true; render(); });
  close.id = 'twClose';
  panel.classList.add(catalog ? 'watcherCatalog' : 'watcherExtension');
  const iconFamily = getComputedStyle(document.documentElement).getPropertyValue('--watcher-icon-family').trim();
  const family = ['futaba', 'burichan', 'tomorrow', 'photon'].includes(iconFamily) ? iconFamily : 'futaba';
  const highDensity = matchMedia('(min-resolution: 2dppx)');
  function icon(control, name, description) {
    control.setAttribute('aria-label', description);
    control.title = description;
    if (catalog) {
      control.classList.remove('watchIcon', 'unwatchIcon', 'refreshIcon', 'rotateIcon', 'closeIcon');
      control.classList.add({ watch_thread_off: 'watchIcon', watch_thread_on: 'unwatchIcon',
        refresh: 'refreshIcon', post_expand_rotate: 'rotateIcon', cross: 'closeIcon' }[name]);
    } else {
      let image = control.querySelector('img');
      if (!image) {
        image = node('img');
        image.alt = '';
        image.width = image.height = 18;
        image.draggable = false;
        control.append(image);
      }
      const path = `/static/watcher/${family}/${name}${highDensity.matches ? '@2x' : ''}.${name === 'post_expand_rotate' ? 'gif' : 'png'}`;
      if (image.getAttribute('src') !== path) image.src = path;
    }
  }
  refreshButton.textContent = close.textContent = '';
  refreshButton.classList.add('watcherIcon');
  close.classList.add('watcherIcon');
  highDensity.addEventListener('change', () => render());
  heading.append(close, title, refreshButton);
  for (const control of document.querySelectorAll('[data-thread-refresh]')) {
    control.addEventListener('click', event => {
      if (control.dataset.updaterReady === 'true') return;
      if (event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
      const target = control.dataset.threadRefresh;
      if (target !== 'top' && target !== 'bottom') return;
      event.preventDefault();
      history.replaceState(null, '', `${location.pathname}${location.search}#${target}`);
      location.reload();
    });
  }
  const body = node('div');
  body.id = 'watcher-body';
  const list = node('ul');
  list.id = 'watchList';
  const notice = node('p', '', 'watcherNotice');
  notice.setAttribute('role', 'status');
  body.append(list, notice);
  panel.append(heading, body);
  document.body.append(panel);

  const threadRefresh = new WatcherRefresh({ origin: location.origin, getEntries: () => entries,
    getTracked: key => tracking.tracked(key),
    commit: (key, expected, next, signal) => change(rows => {
      if (!sameEntry(rows.get(key), expected)) return false;
      if (next) rows.set(key, next); else rows.delete(key);
    }, signal),
  });
  const catalogTransport = new NativeCatalogTransport();
  const matcher = new NativeFilterMatcher();
  let refreshCycle = null;
  const refresh = {
    cancel() { refreshCycle?.abort(); catalogTransport.cancel(); threadRefresh.cancel(); },
    refresh: options => threadRefresh.refresh(options),
  };
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

  let nativeReplies = null, nativeThreads = null, nativeQuotePreview = null, nativeBacklinks = null;
  nativeBacklinks = catalog ? null : mountNativeBacklinks({ root: document.querySelector('.board'),
    board, thread: threadId, settings: configuration, mobile, readNeverMobile, quoteTarget,
    changed: () => { nativeQuotePreview?.refresh(); if (nativeBacklinks) syncPostMenus(); },
  });
  const nativeFilters = catalog ? null : mountNativeFilters({ board, threadId, settings: configuration,
    read: () => read(filterKey), save: saveFilterRules,
    match: (...args) => matcher.match(...args), getTracked: key => tracking.tracked(key),
    changed: () => refresh.cancel(),
    commentHTML: message => nativeBacklinks?.commentHTML(message) ?? message.innerHTML,
    applied: hidden => { nativeReplies?.setFiltered(hidden); nativeThreads?.setFiltered(hidden); },
  });
  nativeReplies = catalog ? null : mountNativeReplyHiding({ board, settings: configuration, changed: syncOpenPostMenu });
  nativeThreads = catalog ? null : mountNativeThreadHiding({ board, threadId, settings: configuration, changed: syncOpenPostMenu });
  const nativeLinkification = catalog ? null : mountNativeLinkification({
    root: document.querySelector('.board'), settings: configuration, mobile, readNeverMobile,
  });
  nativeQuotePreview = catalog ? null : mountNativeQuotePreview({
    root: document.querySelector('.board'), board, thread: threadId, mediaOrigin: context.dataset.mediaOrigin,
    settings: configuration, decorate: () => nativeLinkification?.refresh(),
    companion: link => nativeBacklinks?.companion(link),
    decoratePreview: (...args) => nativeBacklinks?.decoratePreview(...args),
  });
  const nativeUpdater = catalog ? null : mountNativeThreadUpdater({ board, thread: threadId,
    worksafe: context.dataset.worksafe === 'true', mediaOrigin: context.dataset.mediaOrigin, settings: configuration,
    applied: async (_snapshot, signal) => {
      render();
      // Let insertion/quote-decoration observers enqueue their refresh first.
      // Follow replacement filter passes before deciding notification priority.
      await new Promise(resolve => queueMicrotask(resolve));
      if (signal.aborted) return;
      if (nativeFilters && !await nativeFilters.refreshSettled(signal)) {
        if (!signal.aborted) throw new Error('Filters did not settle after the update.');
        return;
      }
      if (signal.aborted) return;
      const acknowledgement = new AbortController();
      const cancel = () => acknowledgement.abort();
      signal.addEventListener('abort', cancel, { once: true });
      const timer = setTimeout(cancel, 1000);
      try {
        const saved = await acknowledgeCurrent(acknowledgement.signal);
        if (saved === false && !signal.aborted && entries.has(watchKey(board, threadId))) {
          notice.textContent = 'Thread updated. Watch read position could not be saved.';
        }
      } finally { clearTimeout(timer); signal.removeEventListener('abort', cancel); acknowledgement.abort(); }
    },
  });
  const nativeQuickReply = catalog ? null : mountNativeQuickReply({ board, thread: threadId, settings: configuration,
    savePosition: position => saveSettings({ 'QR-position': position }),
    committed: (id, post) => {
      const saved = tracking.committed(id, post).catch(() => false);
      nativeUpdater?.posted(post, saved);
      return saved.then(() => { render(); });
    },
  });
  const nativeKeys = catalog ? null : mountNativeKeybinds({ board, settings: configuration,
    quickReply: () => nativeQuickReply?.open(),
    update: () => { void nativeUpdater?.update(); },
    auto: () => nativeUpdater?.toggleAuto(),
    watch: () => { if (enabled && threadId) void toggleThread(document.getElementById(`t${threadId}`)); },
    filter: () => { if (configuration().filter === true) nativeFilters?.addSelection(document.activeElement, nativeFilters.selection()); },
  });
  const settingsNavigation = installSettings({ catalog, read: configuration, save: saveSettings,
    openFilters: opener => nativeFilters?.open(opener),
    clearThreads: () => { void nativeThreads?.clearHistory(); },
    openKeybinds: opener => nativeKeys?.openHelp(opener),
    optionChecked: (key, initial) => key === 'linkify'
      ? (initial.disableAll === true ? initial.linkify === true
        : (mobile.matches && readNeverMobile() !== 'true') || initial.linkify === true)
      : undefined,
    toggleWatcher: () => { collapsed = !collapsed; render(); if (!collapsed) void refreshAll(true); },
  });
  const placement = mountWatcherPosition({ panel, heading, catalog, mobile, read: configuration,
    save: (position, expected, expectedFixed) => locked(() => {
      const settings = configuration();
      if (!enabled || settings.disableAll === true || settings.threadWatcher !== true
        || JSON.stringify(settings['TW-position']) !== expected
        || (settings.fixedThreadWatcher === true) !== expectedFixed) return false;
      return writeSettings({ ...settings, 'TW-position': position });
    }),
  });
  function writeSettings(settings) {
    const raw = JSON.stringify(settings);
    if (raw.length > 4096) { notice.textContent = 'Settings are too large to save this change.'; return false; }
    if (persistent) {
      try { localStorage.setItem(settingsKey, raw); }
      catch { persistent = false; volatileSettings = true; }
    } else volatileSettings = true;
    settingsCache = settings;
    return true;
  }
  function saveFilterRules(raw, expected, signal) {
    return locked(() => {
      if (signal.aborted || read(filterKey) !== expected) return { status: 'conflict' };
      if (persistent) {
        try {
          if (raw === null) localStorage.removeItem(filterKey);
          else localStorage.setItem(filterKey, raw);
        } catch { persistent = false; }
      }
      if (!persistent) { volatileFilters = true; filterCache = raw; }
      refresh.cancel();
      return { status: 'ok', persisted: persistent };
    }, signal);
  }
  async function saveSettings(changes) {
    const applied = await locked(() => {
      const settings = { ...configuration(), ...changes };
      if (catalog && changes.threadWatcher === true) settings.disableAll = false;
      if (!writeSettings(settings)) return false;
      refresh.cancel();
      enabled = settings.threadWatcher === true && settings.disableAll !== true;
      collapsed = mobile.matches;
      render();
      return true;
    });
    if (applied === false || !mutationLock.active) return false;
    if (enabled) { await acknowledgeCurrent(); if (!mutationLock.active) return false; navigateReadPosition(); }
    void nativeFilters?.refresh();
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
    // The source text catalog has no watch buttons on its rows.
    if (catalog && document.getElementById('threads')?.dataset.textOnly === 'true') return [];
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
    }, undefined, catalog ? null : key);
  }
  function controls() {
    for (const section of sections()) {
      const id = sectionId(section);
      if (!id) continue;
      const watched = entries.has(watchKey(board, id));
      const update = control => {
        control.hidden = !enabled;
        icon(control, watched ? 'watch_thread_on' : 'watch_thread_off', `${watched ? 'Unwatch' : 'Watch'} thread ${id}`);
        control.title = catalog ? (watched ? 'Unwatch' : 'Watch') : 'Add to watch list';
        control.setAttribute('aria-pressed', String(watched));
        control.dataset.id = id;
        control.dataset.cmd = 'watch';
        if (watched) control.dataset.active = '1'; else delete control.dataset.active;
      };
      if (threadId && !catalog) {
        for (const nav of document.querySelectorAll('.threadNav')) {
          let wrapper = nav.querySelector('.watcherNavControl');
          if (!wrapper && !enabled) continue;
          if (!wrapper) {
            const compact = nav.classList.contains('mobile');
            wrapper = node('span', undefined, compact ? 'mobileib button watcherNavControl' : 'watcherNavControl');
            const control = button('', () => toggleThread(section), `wbtn watcherIcon wbtn-${id}-${board}`);
            control.id = `wbtn-${id}-${nav.dataset.watchPosition}`;
            if (compact) { wrapper.append(control); nav.append(' ', wrapper); }
            else { wrapper.append('[', control, '] '); nav.prepend(wrapper); }
          }
          wrapper.hidden = !enabled;
          update(wrapper.querySelector('.wbtn'));
        }
      } else if (catalog) {
        let control = section.querySelector('.wbtn');
        if (!control) {
          control = button('', () => toggleThread(section), 'wbtn watcherIcon');
          control.id = `leaf-${id}`;
          section.prepend(control);
        }
        update(control);
      }
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
    if (!catalog) syncPostMenus();
  }
  function closePostMenu(restoreFocus = false) {
    if (!activePostMenu) return;
    const { root, trigger } = activePostMenu;
    activePostMenu = null;
    root.remove();
    trigger.classList.remove('menuOpen');
    trigger.setAttribute('aria-expanded', 'false');
    if (restoreFocus && trigger.isConnected && !trigger.hidden) trigger.focus();
  }
  function positionPostMenu(menu) {
    const rect = menu.trigger.getBoundingClientRect();
    const right = Math.max(0, document.documentElement.clientWidth - menu.root.offsetWidth);
    menu.root.classList.toggle('dd-menu-left', rect.left > right - 75);
    menu.root.style.top = `${rect.bottom + 3 + window.scrollY}px`;
    menu.root.style.left = `${Math.max(0, Math.min(rect.left, right)) + window.scrollX}px`;
  }
  function postMenuItem(menu, command, text, action) {
    const item = node('li');
    item.setAttribute('role', 'none');
    const control = button(text, () => {
      closePostMenu(command === 'watch' || command === 'hide-r' || command === 'hide');
      action();
    });
    control.setAttribute('role', 'menuitem');
    control.dataset.cmd = command;
    control.dataset.id = menu.post.id.slice(1);
    item.append(control);
    menu.list.append(item);
    return control;
  }
  function postMenuLink(list, text, href) {
    const item = node('li'); item.setAttribute('role', 'none');
    const link = node('a', text);
    link.href = href; link.target = '_blank'; link.rel = 'noopener noreferrer';
    link.tabIndex = -1; link.setAttribute('role', 'menuitem');
    link.addEventListener('click', () => closePostMenu(true));
    item.append(link); list.append(item);
    return link;
  }
  function postFileMenu(menu) {
    const source = menu.post.querySelector('.file > p > a[href]');
    if (!source) return;
    let file;
    try { file = new URL(source.href); } catch { return; }
    if (!['http:', 'https:'].includes(file.protocol) || file.username || file.password || file.search || file.hash
      || !new RegExp(`^/${board}/[1-9][0-9]{0,18}\\.png$`).test(file.pathname)) return;
    const providers = [
      ['Google', 'https://lens.google.com/uploadbyurl'],
      ['Yandex', 'https://www.yandex.com/images/search'],
      ['SauceNAO', 'https://saucenao.com/search.php'],
    ].map(([name, endpoint]) => {
      const url = new URL(endpoint); url.searchParams.set('url', file.href);
      if (name === 'Yandex') url.searchParams.set('rpt', 'imageview');
      return [name, url.href];
    });
    if (mobile.matches) {
      if (menu.post.querySelector(`form[action="/${board}/delete"] input[name=file_only]`)) {
        postMenuItem(menu, 'del-file', 'Delete file', () => openPostAction(menu.post, 'delete', true));
      }
      postMenuLink(menu.list, 'Open normalized file', file.href);
      for (const [name, url] of providers) postMenuLink(menu.list, `Search image on ${name}`, url);
      return;
    }
    const item = node('li', undefined, 'imageSearchMenu'); item.setAttribute('role', 'none');
    const submenu = node('ul', undefined, 'imageSearchSubmenu');
    submenu.setAttribute('role', 'menu'); submenu.setAttribute('aria-label', 'Image search providers'); submenu.hidden = true;
    const toggle = button('Image search \u00bb', () => show(submenu.hidden));
    toggle.setAttribute('role', 'menuitem'); toggle.setAttribute('aria-label', 'Image search');
    toggle.setAttribute('aria-haspopup', 'menu'); toggle.setAttribute('aria-expanded', 'false');
    function show(open, focus = false) {
      submenu.hidden = !open; toggle.setAttribute('aria-expanded', String(open));
      if (focus) submenu.querySelector('a')?.focus();
    }
    for (const [name, url] of providers) postMenuLink(submenu, name, url);
    item.addEventListener('pointerenter', () => show(true));
    item.addEventListener('pointerleave', () => { if (!item.contains(document.activeElement)) show(false); });
    item.addEventListener('keydown', event => {
      if (event.key === 'ArrowRight') { event.preventDefault(); event.stopPropagation(); show(true, true); }
      else if (!submenu.hidden && ['ArrowLeft', 'Escape'].includes(event.key)) {
        event.preventDefault(); event.stopPropagation(); show(false); toggle.focus();
      }
    });
    item.append(toggle, submenu); menu.list.append(item);
  }
  function openPostAction(post, action, fileOnly = false) {
    const form = post.querySelector(`.postActions form[action="/${board}/${action}"]`);
    if (!form) return;
    if (action === 'delete') {
      const checkbox = form.querySelector('input[name=file_only]');
      if (checkbox) checkbox.checked = fileOnly;
    }
    const details = form.closest('details');
    if (details) details.open = true;
    form.querySelector(action === 'report' ? '[name="reason"]' : '[name="password"]')?.focus();
  }
  function syncOpenPostMenu(position = true) {
    const menu = activePostMenu;
    if (!menu) return;
    if (!menu.trigger.isConnected || menu.trigger.hidden || !menu.post.isConnected) {
      closePostMenu();
      return;
    }
    const id = sectionId(menu.section);
    if (menu.hide) menu.hide.textContent = `${nativeReplies?.isHidden(menu.post.id.slice(1)) ? 'Unhide' : 'Hide'} post`;
    if (menu.hideThread) {
      menu.hideThread.parentElement.hidden = !nativeThreads?.enabled();
      menu.hideThread.textContent = `${menu.section.classList.contains('post-hidden') ? 'Unhide' : 'Hide'} thread`;
    }
    const canWatch = enabled && menu.post.classList.contains('op') && menu.post.id === `p${id}`;
    if (canWatch) {
      if (!menu.watch) {
        menu.watch = postMenuItem(menu, 'watch', '', () => { void toggleThread(menu.section); });
        menu.list.insertBefore(menu.watch.parentElement, menu.list.children[menu.hideThread ? 2 : 1] || null);
      }
      menu.watch.textContent = `${entries.has(watchKey(board, id)) ? 'Remove from' : 'Add to'} watch list`;
    } else if (menu.watch) {
      const focused = document.activeElement === menu.watch;
      menu.watch.parentElement.remove();
      menu.watch = null;
      if (focused) menu.list.querySelector('[role="menuitem"]')?.focus();
    }
    const canFilter = !catalog && configuration().filter === true;
    if (canFilter && !menu.filter) {
      menu.filter = postMenuItem(menu, 'filter-sel', 'Filter selected text', () => {
        if (configuration().filter === true && configuration().disableAll !== true) {
          nativeFilters?.addSelection(menu.trigger, menu.selection);
        }
      });
    } else if (!canFilter && menu.filter) {
      const focused = document.activeElement === menu.filter;
      menu.filter.parentElement.remove(); menu.filter = null;
      if (focused) menu.list.querySelector('[role="menuitem"]')?.focus();
    }
    if (position) positionPostMenu(menu);
  }
  function openPostMenu(trigger, post, section, focus = null) {
    if (activePostMenu?.trigger === trigger) { closePostMenu(); return; }
    closePostMenu();
    if (configuration().disableAll === true || !post.isConnected) return;
    const root = node('div', undefined, 'dd-menu nativePostMenu');
    root.id = 'post-menu';
    const list = node('ul');
    list.setAttribute('role', 'menu');
    list.setAttribute('aria-label', `Actions for post ${post.id.slice(1)}`);
    root.append(list);
    const menu = { root, list, trigger, post, section, watch: null, filter: null,
      selection: nativeFilters?.selection() };
    postMenuItem(menu, 'report', 'Report post', () => openPostAction(post, 'report'));
    if (!catalog && !threadId && post.classList.contains('op') && nativeThreads?.enabled()) {
      menu.hideThread = postMenuItem(menu, 'hide', '', () => { void nativeThreads.toggle(post.id.slice(1)); });
    }
    if (post.classList.contains('reply')) {
      menu.hide = postMenuItem(menu, 'hide-r', '', () => { void nativeReplies?.toggle(post.id.slice(1)); });
    }
    if (mobile.matches) postMenuItem(menu, 'del-post', 'Delete post', () => openPostAction(post, 'delete'));
    postFileMenu(menu);
    root.addEventListener('keydown', event => {
      const items = [...list.querySelectorAll('[role="menuitem"]')].filter(item => item.getClientRects().length);
      const current = items.indexOf(document.activeElement);
      let index;
      if (event.key === 'ArrowDown') index = (current + 1) % items.length;
      else if (event.key === 'ArrowUp') index = (current - 1 + items.length) % items.length;
      else if (event.key === 'Home') index = 0;
      else if (event.key === 'End') index = items.length - 1;
      else if (event.key === 'Tab') { closePostMenu(true); return; }
      else return;
      event.preventDefault();
      items[index]?.focus();
    });
    activePostMenu = menu;
    trigger.classList.add('menuOpen');
    trigger.setAttribute('aria-expanded', 'true');
    syncOpenPostMenu(false);
    // The pinned client publishes an ordinary, non-bubbling Event before
    // insertion, with the live list available to same-document subscribers.
    const ready = new Event('4chanPostMenuReady');
    ready.detail = { postId: post.id.slice(1),
      isOP: post.classList.contains('op') && sectionId(section) === post.id.slice(1), node: list };
    document.dispatchEvent(ready);
    document.body.append(root);
    positionPostMenu(menu);
    if (focus) {
      const items = [...list.querySelectorAll('[role="menuitem"]')].filter(item => item.getClientRects().length);
      items[focus === 'last' ? items.length - 1 : 0]?.focus();
    }
  }
  function syncPostMenus() {
    const disabled = configuration().disableAll === true;
    for (const section of sections()) {
      for (const post of section.querySelectorAll('.post[id]')) {
        const id = postId(post.id.slice(1));
        const info = post.querySelector('.postInfo');
        if (!id || !info) continue;
        let trigger = info.querySelector('[data-post-menu]');
        if (!trigger && !disabled) {
          trigger = button('', event => {
            event.stopPropagation();
            openPostMenu(trigger, post, section, event.detail === 0 ? 'first' : null);
          }, 'postMenuBtn');
          trigger.dataset.postMenu = id;
          trigger.dataset.cmd = 'post-menu';
          trigger.dataset.family = family;
          trigger.title = 'Post menu';
          trigger.setAttribute('aria-label', `Post menu for post ${id}`);
          trigger.setAttribute('aria-haspopup', 'menu');
          trigger.setAttribute('aria-expanded', 'false');
          trigger.addEventListener('keydown', event => {
            if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
            event.preventDefault();
            if (activePostMenu?.trigger === trigger) closePostMenu();
            openPostMenu(trigger, post, section, event.key === 'ArrowUp' ? 'last' : 'first');
          });
        }
        if (!trigger) continue;
        trigger.hidden = disabled;
        trigger.textContent = mobile.matches ? '...' : '\u25b6';
        if (mobile.matches) {
          if (info.firstElementChild !== trigger) info.prepend(trigger);
        } else {
          const boundary = nativeBacklinks?.menuBoundary(post, info);
          if (boundary) {
            if (trigger.nextElementSibling !== boundary) info.insertBefore(trigger, boundary);
          } else if (info.lastElementChild !== trigger) info.append(trigger);
        }
      }
    }
    syncOpenPostMenu();
  }
  function render() {
    if (!catalog && threadId) markNativeTrackedQuotes(document.getElementById(`t${threadId}`),
      tracking.tracked(watchKey(board, threadId)), configuration().disableAll !== true, nativeBacklinks ?? {});
    nativeBacklinks?.refresh();
    nativeQuotePreview?.refresh();
    nativeUpdater?.sync();
    nativeQuickReply?.sync();
    nativeReplies?.refresh();
    nativeThreads?.refresh();
    tracking.prepareForms();
    panel.hidden = !enabled || (mobile.matches && collapsed);
    close.hidden = !mobile.matches;
    settingsNavigation.setWatcherEnabled(enabled, !panel.hidden);
    body.hidden = collapsed;
    refreshButton.disabled = busy || !enabled;
    icon(refreshButton, busy ? 'post_expand_rotate' : 'refresh', 'Refresh');
    icon(close, 'cross', 'Close');
    panel.setAttribute('aria-busy', String(busy));
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
      const remove = button('\u00d7', async () => {
        refresh.cancel();
        await change(rows => { rows.delete(key); }, undefined, catalog ? null : key);
      }, 'watcherRemove');
      remove.setAttribute('aria-label', `Unwatch /${slug}/ thread ${id}`);
      row.append(remove, ' ', link);
      list.append(row);
    }
    if (!persistent) notice.textContent = 'Storage or cross-tab locking is unavailable. Changes stay in this tab.';
    controls();
    placement.sync();
  }
  async function refreshAll(automatic) {
    if (!enabled || busy || (automatic && document.hidden)) return;
    if (!entries.size && (catalog || automatic || configuration().filter !== true)) return;
    if (automatic && (threadId || !autoRefreshEligible(read(timestampKey), catalog))) return;
    let selection;
    const claimed = await locked(() => {
      load();
      const settings = configuration();
      if (!enabled || settings.threadWatcher !== true || settings.disableAll === true) return false;
      const raw = read(timestampKey);
      if (raw !== null && !autoRefreshEligible(raw, false)) {
        notice.textContent = 'Wait a minute before refreshing again.'; return false;
      }
      if (!catalog && !automatic && settings.filter === true) {
        const filterRaw = read(filterKey);
        const parsed = readNativeFilters(filterRaw);
        const selected = parsed.status === 'ok' ? autoWatchBoards(parsed.filters) : parsed;
        const blocked = loadBlacklist();
        selection = selected.status === 'ok' && blocked
          ? { filters: parsed.filters, boards: selected.boards, filterRaw,
            watches: writeWatches(entries), blacklist: writeBlacklist(blocked) }
          : { invalid: true };
      }
      timestampCache = String(Date.now());
      if (persistent) {
        try { localStorage.setItem(timestampKey, timestampCache); } catch { persistent = false; }
      }
      return true;
    });
    if (!claimed || !enabled || !mutationLock.active) return;
    busy = true;
    render();
    const controller = new AbortController();
    refreshCycle = controller;
    const timer = setTimeout(() => refresh.cancel(), WATCH_LIMITS.cycleMs);
    try {
      let bytes = WATCH_LIMITS.cycleBytes;
      let autoFailed = selection?.invalid ? 1 : 0;
      let limited = 0;
      if (selection?.boards?.length) {
        const cycle = await collectAutoWatches({ filters: selection.filters, boards: selection.boards,
          transport: catalogTransport, matcher, signal: controller.signal });
        bytes -= cycle.bytes;
        if (cycle.status === 'complete' && !controller.signal.aborted) {
          const applied = await locked(() => {
            load();
            const blocked = loadBlacklist();
            const settings = configuration();
            if (controller.signal.aborted || !enabled || settings.threadWatcher !== true
              || settings.disableAll === true || settings.filter !== true
              || read(filterKey) !== selection.filterRaw || writeWatches(entries) !== selection.watches
              || !blocked || writeBlacklist(blocked) !== selection.blacklist) return false;
            const next = planAutoWatches(entries, blocked, cycle);
            // Persist additions before pruning proven-absent blacklist entries.
            // On failure, older suppression remains safe for other tabs.
            if (persistent) {
              try {
                localStorage.setItem(storeKey, writeWatches(next.entries));
                localStorage.setItem(blacklistKey, writeBlacklist(next.blacklist));
              } catch { persistent = false; }
            }
            entries = next.entries;
            blacklistCache = next.blacklist;
            autoFailed = next.failed;
            limited = next.limited;
            render();
            return true;
          }, controller.signal);
          if (!applied) autoFailed++;
        } else autoFailed++;
        // Retain launch spacing across the catalog/thread phase boundary.
        if (!controller.signal.aborted) await new Promise(resolve => {
          const finish = () => { clearTimeout(pause); controller.signal.removeEventListener('abort', finish); resolve(); };
          const pause = setTimeout(finish, WATCH_LIMITS.staggerMs);
          controller.signal.addEventListener('abort', finish, { once: true });
        });
      }
      const result = await refresh.refresh({ signal: controller.signal, bytes });
      const failed = result.results.filter(row => row.status === 'failed').length;
      notice.textContent = result.status === 'cooldown' ? 'Wait a minute before refreshing again.'
        : result.status === 'cancelled' ? 'Refresh stopped.'
          : failed ? `${failed} thread refreshes failed. Saved state was retained.`
            : autoFailed ? 'Some automatic watches could not be refreshed. Existing watches and failed-board blacklists were retained.'
              : limited ? `Refresh complete. Watch limit: ${WATCH_LIMITS.entries} threads.` : 'Refresh complete.';
    } catch {
      notice.textContent = 'Refresh failed. Saved watches were retained.';
    } finally {
      clearTimeout(timer);
      if (refreshCycle === controller) refreshCycle = null;
      busy = false;
      render();
    }
  }
  async function acknowledgeCurrent(signal) {
    if (!enabled || !threadId) return;
    const section = document.getElementById(`t${threadId}`);
    if (!section) return;
    return change(rows => {
      const key = watchKey(board, threadId);
      const current = rows.get(key);
      if (!current) return false;
      rows.set(key, acknowledgedEntry(current, latest(section)));
    }, signal);
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
    if (event.key !== null && ![storeKey, settingsKey, blacklistKey, filterKey].includes(event.key)) return;
    refresh.cancel();
    load();
    const settings = configuration();
    enabled = settings.threadWatcher === true && settings.disableAll !== true;
    render();
    if (event.key === null || event.key === settingsKey || event.key === filterKey) void nativeFilters?.refresh();
  });
  document.addEventListener('click', event => {
    if (activePostMenu && !activePostMenu.root.contains(event.target)
      && !activePostMenu.trigger.contains(event.target)) closePostMenu();
  });
  document.addEventListener('focusin', event => {
    if (activePostMenu && !activePostMenu.root.contains(event.target)
      && !activePostMenu.trigger.contains(event.target)) closePostMenu();
  });
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape' && activePostMenu) {
      event.preventDefault();
      closePostMenu(true);
    }
  });
  window.addEventListener('resize', () => closePostMenu());
  window.addEventListener('pagehide', () => { mutationLock.suspend(); closePostMenu(); refresh.cancel(); });
  window.addEventListener('pageshow', event => { if (event.persisted) mutationLock.resume(); });
  mobile.addEventListener('change', () => { closePostMenu(); collapsed = mobile.matches; render(); });
  const container = document.getElementById('threads');
  if (container) new MutationObserver(controls).observe(container, { childList: true });
  render();
  tracking.consume(threadId).then(async () => {
    if (!mutationLock.active) return;
    await acknowledgeCurrent();
    if (!mutationLock.active) return;
    render();
    void nativeFilters?.refresh();
    navigateReadPosition(); return refreshAll(true);
  });
}
