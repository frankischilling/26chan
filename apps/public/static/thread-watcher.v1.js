import { mountNativeBlotter } from './native-blotter.v1.js';
import { WATCH_LIMITS, reportURL, createReportRegistry, postId, watchKey, splitWatchKey, watchLabel, readWatches, writeWatches,
  sameEntry, orderedWatches, autoRefreshEligible, acknowledgedEntry,
  WatcherRefresh } from './thread-watcher-core.v1.js';
import { PostTracking } from './post-tracking.v1.js';
import { installSettings, catalogDropDownEnabled, captureSettingsPresentation, settingsOptionChecked, settingsStartupDefaults, readSettingsStartup, configuredHTTPSOrigin, httpsPreferenceEnabled, settingsHTTPSRedirect, httpsPreferenceCookie } from './native-settings.v1.js';
import { mountWatcherPosition } from './watcher-position.v1.js';
import { NativeCatalogTransport, NativeFilterMatcher, NativeWatchLock, readNativeFilters, autoWatchBoards, mountNativeFilters, mountNativeReplyHiding, mountNativeThreadHiding, mountNativeKeybinds, markNativeTrackedQuotes,
  readBlacklist, writeBlacklist, collectAutoWatches, planAutoWatches, mountNativeLinkification, mountNativeQuotePreview, quoteTarget,
  localQuoteTree, prepareQuotePost, mobileQuoteDevice, NativeQuotePreviewTransport, checkedQuotePreview } from './native-filter.v1.js';
import { mountNativeBacklinks, mountNativeInlineQuotes, createCommentProjection } from './native-backlinks.v1.js';
import { mountNativeQuickReply } from './native-quick-reply.v1.js';
import { mountNativeImages } from './native-images.v1.js';
import { mountNativeDeletion } from './native-post-deletion.v1.js';
import { mountNativeDisplay, mountNativePosterIds, mountNativePosterIdActions } from './native-display.v1.js';
import { mountNativePostTooltips } from './native-post-tooltips.v1.js';
import { createParsingBootstrap, createInitialMountLifecycle, dispatchSourceEvent, mountNativeThreadUpdater, mountNativeThreadExpansion, mountNativeDepager, NativeBoardPageTransport } from './native-thread-controls.v1.js';
import { mountNativeThreadStats } from './native-thread-stats.v1.js';
import { mountNativeNavigation, navigationPage } from './native-navigation.v1.js';
import { mountNativeLayout, sourceMobileLayout, THEME_READY_EVENT } from './native-layout.v1.js';
import { mountNativeEmbeds } from './native-embeds.v1.js';
import { mountNativeCustomCSS } from './native-custom-css.v1.js';
import { mountNativeSettingsTransfer, checkTransferValues, SETTINGS_TRANSFER_STORAGE_KEYS, SETTINGS_TRANSFER_LIMITS } from './native-settings-transfer.v1.js';
import { CatalogFilterMatcher, readCatalogFilters } from './catalog-filter-core.v1.js';

const context = document.getElementById('watcher-context');
if (context && watchKey(context.dataset.board, '1')) start(context);

async function start(context) {
  const httpsOrigin = configuredHTTPSOrigin(context.dataset.publicOrigin, location.href);
  function readHTTPSCookie() {
    try { return document.cookie; } catch { return ''; }
  }
  let startupRaw = null;
  try { startupRaw = localStorage.getItem('4chan-settings'); } catch { /* Unknown storage cannot enable navigation. */ }
  const httpsRedirect = settingsHTTPSRedirect(httpsOrigin, location.href, startupRaw, readHTTPSCookie());
  if (httpsRedirect) { location.assign(httpsRedirect); return; }
  const board = context.dataset.board;
  const threadId = postId(context.dataset.thread);
  const catalog = context.dataset.catalog === 'true';
  const mountLifecycle = createInitialMountLifecycle(window);
  let initialParsing = new AbortController(), initialSuspended = false;
  const cancelParsing = () => { initialSuspended = true; initialParsing.abort(); };
  if (!catalog) {
    window.addEventListener('pagehide', cancelParsing);
    window.addEventListener('pageshow', () => { initialSuspended = false; });
  }
  const parsingBootstrap = createParsingBootstrap(configuration);
  const storeKey = '4chan-watch';
  const settingsKey = '4chan-settings';
  const timestampKey = '4chan-tw-timestamp';
  const blacklistKey = '4chan-watch-bl';
  const filterKey = '4chan-filters';
  const cssKey = '4chan-css';
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
        configuration(); filterCache = read(filterKey); cssCache = read(cssKey);
        persistent = false; volatileSettings = true; volatileFilters = true; volatileCSS = true;
        tracking.persistent = false;
        mutationLock.acquire = null;
        return action();
      }
    } : null,
    warn: text => { notice.textContent = text; },
  });
  let settingsStartupRead;
  let settingsStartupState = null;
  let settingsCache = {};
  let settingsRawCache = null;
  let volatileSettings = false;
  let volatileFilters = false;
  let filterCache = null;
  let volatileCSS = false;
  let cssCache = null;
  let timestampCache = null;
  const mobile = matchMedia('(max-width: 480px)');
  const settingsStartupLayout = sourceMobileLayout(mobile.matches, readNeverMobile());
  let entries = readWatches(read(storeKey));
  let enabled = configuration().threadWatcher === true && configuration().disableAll !== true;
  let busy = false;
  let blacklistCache = new Set();
  let invalidBlacklist = false;
  let activePostMenu = null;
  let collapsed = mobile.matches;
  const settingsPresentation = captureSettingsPresentation(settingsStartupRead, settingsStartupLayout);
  // Runtime defaults and guarded persistence are independent of presentation.
  // Only a successful absent/empty startup read establishes this state.
  if (!catalog && settingsPresentation.firstRun) settingsStartupState = {
    active: true, mobileLayout: settingsStartupLayout, mobileDevice: mobileQuoteDevice(navigator.userAgent),
  };
  // Raw storage is always read afresh before any first-open persistence.
  settingsStartupRead = { status: 'captured' };

  // Main.init publishes context and preferences before any parser starts.
  // Upload forms share watcher storage but are not board parsing documents.
  if (!catalog && document.querySelector('.board')) {
    mountNativeBlotter(document);
    dispatchSourceEvent(document, '4chanMainInit');
  }
  let mathModule = null;
  if (!catalog && document.body.dataset.mathTags === '1') {
    try { mathModule = await import('./native-math.v1.js'); } catch { /* Literal tags remain usable. */ }
  }
  // A resolved import must not mount into a suspended or departed document.
  // Recheck after every wake: a second pagehide can precede this continuation.
  while (!mountLifecycle.active()) {
    if (!await mountLifecycle.wait()) { mountLifecycle.disconnect(); return; }
  }
  mountLifecycle.disconnect();
  let mathPage = null;
  try { mathPage = mathModule?.pageNativeMath(); } catch { /* Literal tags remain usable. */ }
  const projection = mathPage?.projection ?? createCommentProjection();
  const nativeMath = mathPage?.controller;

  function readNeverMobile() {
    try { return localStorage.getItem('4chan_never_show_mobile'); }
    catch { return null; }
  }

  function read(key) {
    if (key === filterKey && volatileFilters) return filterCache;
    if (key === cssKey && volatileCSS) return cssCache;
    if (key === timestampKey && !persistent) return timestampCache;
    try {
      const value = localStorage.getItem(key);
      if (key === timestampKey) timestampCache = value;
      return value;
    }
    catch { persistent = false; return key === timestampKey ? timestampCache : null; }
  }
  function configuration() {
    if (volatileSettings) return effectiveSettings(settingsCache);
    let raw;
    try { raw = localStorage.getItem(settingsKey); }
    catch {
      settingsStartupRead ??= { status: 'unavailable' };
      persistent = false; volatileSettings = true; return effectiveSettings(settingsCache);
    }
    settingsStartupRead ??= { status: 'ok', raw };
    settingsRawCache = raw;
    settingsCache = {};
    if (raw && raw.length <= 4096) {
      try {
        const value = JSON.parse(raw);
        if (value && typeof value === 'object' && !Array.isArray(value)) settingsCache = value;
      } catch { /* Malformed preferences use finite defaults. */ }
    }
    return effectiveSettings(settingsCache);
  }
  function effectiveSettings(settings) {
    const defaults = settingsStartupState?.active
      ? settingsStartupDefaults({ ...settingsStartupState, disabled: settings.disableAll === true }) : {};
    return { ...defaults, ...settings,
      ...(httpsOrigin ? { forceHTTPS: httpsPreferenceEnabled(settingsRawCache, readHTTPSCookie()) } : {}) };
  }
  function initializeSettingsOnOpen(signal) {
    const startup = settingsStartupState;
    if (!startup?.active || signal?.aborted) return Promise.resolve({ status: 'skipped' });
    return locked(() => {
      if (signal?.aborted || !startup.active || location.hash.startsWith('#cfg=')
        || settingsTransfer?.hasPendingReview()) return { status: 'skipped' };
      // A captured firstRun flag is never authority to repair unknown raw data
      // or replace a newer preference snapshot from another tab.
      let raw;
      try { raw = localStorage.getItem(settingsKey); }
      catch { return { status: 'skipped' }; }
      const current = readSettingsStartup(raw);
      if (current.status !== 'ok') return { status: 'skipped' };
      const settings = effectiveSettings(volatileSettings ? { ...current.settings, ...settingsCache } : current.settings);
      if (JSON.stringify(settings).length > 4096) return { status: 'skipped' };
      if (!writeSettings(settings)) return false;
      return { status: 'ok', persisted: persistent && !volatileSettings };
    }, signal);
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
  function themeFamily() {
    const value = getComputedStyle(document.documentElement).getPropertyValue('--watcher-icon-family').trim().replace(/['"]/g, '');
    return ['futaba', 'burichan', 'tomorrow', 'photon'].includes(value) ? value : 'futaba';
  }
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
      const path = `/static/watcher/${themeFamily()}/${name}${highDensity.matches ? '@2x' : ''}.${name === 'post_expand_rotate' ? 'gif' : 'png'}`;
      if (image.getAttribute('src') !== path) image.src = path;
    }
  }
  refreshButton.textContent = close.textContent = '';
  refreshButton.classList.add('watcherIcon');
  close.classList.add('watcherIcon');
  highDensity.addEventListener('change', () => { render(); nativeNavigation?.themeChanged(); });
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
  const catalogMatcher = new CatalogFilterMatcher();
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

  let nativeReplies = null, nativeThreads = null, nativeQuotePreview = null, nativeBacklinks = null, nativeInlineQuotes = null;
  nativeBacklinks = catalog ? null : mountNativeBacklinks({ root: document.querySelector('.board'),
    board, thread: threadId, settings: configuration, mobile, readNeverMobile, quoteTarget, projection,
    changed: () => { nativeQuotePreview?.refresh(); if (nativeBacklinks) syncPostMenus(); },
  });
  const nativeFilters = catalog ? null : mountNativeFilters({ board, threadId, settings: configuration, projection,
    headerForPost: post => post.querySelector(sourceMobileLayout(mobile.matches, readNeverMobile())
      ? ':scope > .postInfoM' : ':scope > .postInfo'),
    read: () => read(filterKey), save: saveFilterRules,
    match: (...args) => matcher.match(...args), getTracked: key => tracking.tracked(key),
    changed: () => refresh.cancel(),
    commentHTML: message => nativeBacklinks?.commentHTML(message) ?? projection.html(message),
    applied: hidden => { nativeReplies?.setFiltered(hidden); nativeThreads?.setFiltered(hidden); },
  });
  nativeReplies = catalog ? null : mountNativeReplyHiding({ board, settings: configuration, changed: syncOpenPostMenu });
  nativeThreads = catalog ? null : mountNativeThreadHiding({ board, threadId, settings: configuration, changed: syncOpenPostMenu });
  const nativeLinkification = catalog ? null : mountNativeLinkification({
    root: document.querySelector('.board'), settings: configuration, mobile, readNeverMobile, projection,
    beforeTransform: message => nativeMath?.restoreMessage(message),
  });
  nativeInlineQuotes = catalog ? null : mountNativeInlineQuotes({
    root: document.querySelector('.board'), board, thread: threadId, mediaOrigin: context.dataset.mediaOrigin,
    settings: configuration, archive: !!document.querySelector('.archiveNotice'), mobileDevice: mobileQuoteDevice(navigator.userAgent),
    projection, quoteTarget, localQuoteTree, prepareQuotePost, checkedQuotePreview,
    transport: new NativeQuotePreviewTransport({ mediaOrigin: context.dataset.mediaOrigin }),
    companion: link => nativeQuotePreview?.companion(link) ?? nativeBacklinks?.companion(link),
    backlinkOwner: link => nativeBacklinks?.backlinkOwner(link),
    prepareBacklinks: (...args) => nativeBacklinks?.prepareInlineCopy(...args),
    registerMath: (...args) => nativeMath?.registerQuote(...args),
  });
  nativeQuotePreview = catalog ? null : mountNativeQuotePreview({
    root: document.querySelector('.board'), board, thread: threadId, mediaOrigin: context.dataset.mediaOrigin,
    settings: configuration, decorate: () => nativeLinkification?.refresh(), projection,
    arbitrateClick: event => nativeInlineQuotes?.click(event) ?? 'passpreview',
    inlineHoverEligible: link => nativeInlineQuotes?.hoverEligible(link) === true,
    quoteContext: link => nativeInlineQuotes?.quoteContext(link),
    companion: link => nativeBacklinks?.companion(link) ?? nativeInlineQuotes?.companion(link),
    decoratePreview: (...args) => nativeBacklinks?.decoratePreview(...args),
    registerMath: (...args) => nativeMath?.registerQuote(...args),
  });
  const nativeImages = catalog ? null : mountNativeImages({ root: document.querySelector('.board'),
    mediaOrigin: context.dataset.mediaOrigin, settings: configuration, projection, mobile, family: themeFamily,
    previewRoot: () => document.getElementById('quote-preview'),
  });
  const nativeDeletion = catalog ? null : mountNativeDeletion({ root: document.querySelector('.board'), board,
    settings: configuration, mobileLayout: () => sourceMobileLayout(mobile.matches, readNeverMobile()),
    projection, images: nativeImages,
    complete: post => { if (activePostMenu?.post === post) closePostMenu(); },
  });
  const nativeUpdater = catalog ? null : mountNativeThreadUpdater({ board, thread: threadId,
    worksafe: context.dataset.worksafe === 'true', mediaOrigin: context.dataset.mediaOrigin, settings: configuration, ready: parsingBootstrap.ready, projection,
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
  const nativeQuickReply = catalog ? null : mountNativeQuickReply({ board, thread: threadId, settings: configuration, math: nativeMath,
    savePosition: position => saveSettings({ 'QR-position': position }),
    committed: (id, post) => {
      const saved = tracking.committed(id, post).catch(() => false);
      nativeUpdater?.posted(post, saved);
      return saved.then(() => { render(); });
    },
  });
  const nativeKeys = catalog ? null : mountNativeKeybinds({ board, settings: () => document.documentElement.dataset.nativeDrawingActive === 'true'
    ? { ...configuration(), keyBinds: false } : configuration(),
    quickReply: () => nativeQuickReply?.open(),
    update: () => { void nativeUpdater?.update(); },
    auto: () => nativeUpdater?.toggleAuto(),
    watch: () => { if (enabled && threadId) void toggleThread(document.getElementById(`t${threadId}`)); },
    filter: () => { if (configuration().filter === true) nativeFilters?.addSelection(document.activeElement, nativeFilters.selection()); },
  });
  let nativeEmbeds = null, nativeCustomCSS = null, settingsTransfer = null;
  const catalogTheme = catalog ? import('./catalog-theme.v1.js').then(({ catalogMainBootstrap, mountCatalogTheme }) => catalogMainBootstrap(context).settings(() => () => mountCatalogTheme({
    root: context, configuration, saveVolatileSettings: (changes, signal) => saveSettings(changes, signal, true),
    mobileLayout: () => mobile.matches && readNeverMobile() !== 'true',
  }))).catch(() => { notice.textContent = 'Catalog settings could not be loaded.'; return null; }) : null;
  const settingsNavigation = installSettings({ catalog, read: configuration, save: saveSettings,
    httpsAvailable: !!httpsOrigin,
    initializeOnOpen: initializeSettingsOnOpen,
    openCatalogSettings: catalog ? opener => { void catalogTheme.then(controller => {
      if (controller) controller.open(opener); else notice.textContent = 'Catalog settings could not be opened.';
    }); } : undefined,
    presentation: settingsPresentation,
    openFilters: opener => nativeFilters?.open(opener),
    clearThreads: () => { void nativeThreads?.clearHistory(); },
    openKeybinds: opener => nativeKeys?.openHelp(opener),
    openCustomMenu: opener => nativeDisplay?.openEditor(opener),
    openCustomCSS: document.querySelector('.board') ? opener => nativeCustomCSS?.open(opener) : undefined,
    openExport: opener => settingsTransfer?.openExport(opener),
    optionChecked: settingsOptionChecked,
    toggleWatcher: () => { collapsed = !collapsed; render(); if (!collapsed) void refreshAll(true); },
  });
  const nativePosterIds = catalog ? null : mountNativePosterIds({ root: document.body, settings: configuration });
  const nativePosterIdActions = catalog ? null : mountNativePosterIdActions({ root: document.body,
    settings: configuration, thread: Boolean(threadId) });
  const nativeDisplay = mountNativeDisplay({ root: document.body,
    settings: configuration, save: saveSettings, openSettings: opener => settingsNavigation.open(opener), projection,
  });
  const nativePostTooltips = catalog ? null : mountNativePostTooltips({ root: document.body,
    context: { origin: location.origin, board, mediaOrigin: context.dataset.mediaOrigin },
    settings: configuration, projection, display: nativeDisplay,
  });
  const nativeStats = catalog ? null : mountNativeThreadStats({ board, thread: threadId,
    settings: configuration, mobile, readNeverMobile,
  });
  let nativeLayout = null, nativeNavigation = null;
  const themeChanged = () => {
    nativePostTooltips?.clear();
    nativeQuotePreview?.clear();
    render();
    nativeNavigation?.themeChanged();
  };
  document.addEventListener(THEME_READY_EVENT, themeChanged);
  nativeLayout = catalog ? null : mountNativeLayout({ root: document.body, settings: configuration,
    mobile, readNeverMobile, themeStylesheet: document.querySelector('link[data-native-theme-stylesheet]'),
  });
  nativeNavigation = mountNativeNavigation({ root: document.body, board, thread: threadId, catalog,
    settings: () => {
      const settings = configuration();
      return catalog ? { ...settings, dropDownNav: catalogDropDownEnabled(volatileSettings
        ? (settingsRawCache === null && !Object.hasOwn(settings, 'dropDownNav') ? null : JSON.stringify(settings)) : read(settingsKey),
        mobile.matches && readNeverMobile() !== 'true') } : settings;
    }, mobile, readNeverMobile,
    openSettings: opener => settingsNavigation.open(opener), openCustomMenu: opener => nativeDisplay?.openEditor(opener),
    decorateButton: (control, name, description) => {
      control.title = description; control.setAttribute('aria-label', description);
      let image = control.querySelector('img');
      if (!image) { image = document.createElement('img'); image.alt = ''; image.width = image.height = 18; control.append(image); }
      image.src = `/static/navigation/${themeFamily()}/${name}${highDensity.matches ? '@2x' : ''}.png`;
    },
    savePosition: (key, position, expected, signal) => locked(() => {
      const current = configuration();
      if (!['TN-position', 'SN-position'].includes(key) || current.disableAll === true
        || current[key === 'TN-position' ? 'topPageNav' : 'stickyNav'] !== true
        || (typeof current[key] === 'string' ? current[key] : null) !== expected) return false;
      return writeSettings({ ...current, [key]: position });
    }, signal),
  });
  document.addEventListener('click', event => {
    if (event.defaultPrevented || event.button !== 0 || event.shiftKey || event.ctrlKey || event.altKey || event.metaKey
      || configuration().darkTheme !== true) return;
    const link = event.target.closest?.('a');
    if (!link || link.target || link.hasAttribute('download')) return;
    const href = link.getAttribute('href');
    if (!/^\/settings\/theme(?:\?worksafe=(?:true|false))?$/.test(href ?? '')) return;
    event.preventDefault();
    void saveSettings({ darkTheme: false }).then(result => {
      if (result !== false) { nativeLayout?.refresh(); window.location.assign(href); }
      else notice.textContent = 'The style preference could not be updated. Try again.';
    }).catch(() => { notice.textContent = 'The style preference could not be updated. Try again.'; });
  });
  const nativeExpansion = catalog ? null : mountNativeThreadExpansion({ root: document.querySelector('.board'),
    board, thread: threadId, mediaOrigin: context.dataset.mediaOrigin, settings: configuration, ready: parsingBootstrap.ready, projection, decorateButton: icon,
    applied: async (_snapshot, signal) => {
      render(); await new Promise(resolve => queueMicrotask(resolve));
      if (signal.aborted) return;
      if (nativeFilters && !await nativeFilters.refreshSettled(signal) && !signal.aborted) throw new Error('Expansion filters did not settle.');
    },
  });
  const nativeDepager = setupDepager();
  nativeEmbeds = catalog ? null : mountNativeEmbeds({ root: document.querySelector('.board'),
    settings: configuration, hasMobileLayout: () => sourceMobileLayout(mobile.matches, readNeverMobile()), projection,
  });
  nativeCustomCSS = catalog ? null : mountNativeCustomCSS({ root: document.querySelector('.board'),
    settings: configuration, readCSS: () => read(cssKey), saveCSS: saveCustomCSS,
  });
  settingsTransfer = mountNativeSettingsTransfer({ root: document.body,
    httpsAvailable: !!httpsOrigin,
    readItem: readTransferItem, restore: restorePreferences,
  });
  function setupDepager() {
    const page = navigationPage(window.location.pathname, board), pages = document.querySelector('nav.pages');
    const root = document.querySelector('.board');
    if (catalog || threadId || page === null || !pages || !root) return null;
    const next = navigationPage(pages.querySelector('[rel="next"]')?.getAttribute('href'), board);
    const mobileLayout = () => sourceMobileLayout(mobile.matches, readNeverMobile());
    const controls = node('span', undefined, 'nativeDepagerControls');
    const more = button(mobileLayout() ? 'Load More' : 'All', () => {
      if (!controller) return;
      if (!mobileLayout()) {
        overrideAuto = !controller.stats().auto;
        controller.refresh();
        if (!overrideAuto) return;
      }
      void controller.loadMore();
    });
    more.id = 'depage'; more.setAttribute('aria-label', 'Load more threads');
    const cancel = button('Cancel loading', () => controller?.cancel()); cancel.id = 'depage-cancel'; cancel.hidden = true;
    const status = node('span', '', 'nativeDepagerStatus'); status.id = 'depage-status'; status.setAttribute('role', 'status');
    controls.append(' [', more, '] ', cancel, status); pages.append(controls);
    let controller = null, overrideAuto = null, lastAlways = configuration().alwaysDepage === true;
    controller = mountNativeDepager({ root, board, page, nextPage: next, ready: parsingBootstrap.ready, mediaOrigin: context.dataset.mediaOrigin,
      settings: () => {
        const config = configuration(), always = config.alwaysDepage === true;
        if (lastAlways !== always) { overrideAuto = null; lastAlways = always; }
        return { ...config, alwaysDepage: overrideAuto ?? always };
      },
      createTransport: config => new NativeBoardPageTransport(config),
      stateChanged: state => {
        const loading = state.state === 'loading' || state.state === 'applying';
        controls.hidden = state.state === 'disabled' || (state.complete && next === null);
        more.textContent = mobileLayout() ? 'Load More' : 'All';
        more.disabled = loading || (state.complete && (mobileLayout() || !state.auto));
        more.setAttribute('aria-pressed', String(state.auto)); cancel.hidden = !loading;
        status.textContent = { loading: ' Loading next page...', applying: ' Applying page filters...', error: ' Page unavailable. Retry or use Next.',
          limit: ' Page limit reached. Use the ordinary page links to continue.', complete: ' Done.', paused: ' Loading paused.' }[state.state] ?? '';
      },
      applied: async (_page, signal) => {
        render(); await new Promise(resolve => queueMicrotask(resolve));
        if (signal.aborted) return;
        if (nativeFilters && !await nativeFilters.refreshSettled(signal) && !signal.aborted) throw new Error('Page filters did not settle.');
      },
    });
    return controller;
  }
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
    if (persistent && !volatileSettings) {
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
  function saveCustomCSS(raw, expected, signal) {
    if (typeof raw !== 'string' || raw.length > 16384 || new TextEncoder().encode(raw).byteLength > 16384) {
      return Promise.resolve({ status: 'invalid' });
    }
    return locked(() => {
      if (signal.aborted || read(cssKey) !== expected) return { status: 'conflict' };
      if (persistent) {
        try {
          if (raw === '') localStorage.removeItem(cssKey);
          else localStorage.setItem(cssKey, raw);
        }
        catch { persistent = false; }
      }
      if (!persistent) { volatileCSS = true; cssCache = raw === '' ? null : raw; }
      return { status: 'ok', persisted: persistent };
    }, signal);
  }
  function readTransferItem(key) {
    if (!SETTINGS_TRANSFER_STORAGE_KEYS.includes(key)) throw new TypeError('Unsupported preference key');
    if (key === settingsKey && volatileSettings) return JSON.stringify(settingsCache);
    if (key === filterKey && volatileFilters) return filterCache;
    if (key === cssKey && volatileCSS) return cssCache;
    return localStorage.getItem(key);
  }
  async function restorePreferences(values, expected, signal) {
    const checked = checkTransferValues(values);
    if (checked.status !== 'ok' || !expected || typeof expected !== 'object' || Array.isArray(expected)) {
      return Promise.resolve({ status: 'invalid' });
    }
    const next = Object.freeze({ ...checked.values });
    const keys = Object.keys(next);
    const expectedKeys = Object.keys(expected);
    if (expectedKeys.length !== keys.length || keys.some(key => !Object.hasOwn(expected, key)
      || (expected[key] !== null && (typeof expected[key] !== 'string' || expected[key].length > SETTINGS_TRANSFER_LIMITS.existingValueChars)))) {
      return Promise.resolve({ status: 'invalid' });
    }
    const previous = Object.freeze({ ...expected });
    if (signal?.aborted || !mutationLock.active) return { status: 'conflict' };
    if (Object.hasOwn(next, filterKey)) {
      // Use the editor's disposable-worker syntax check. An empty post batch
      // compiles active patterns without running them against post text.
      const parsed = readNativeFilters(next[filterKey]);
      const validation = await matcher.match(parsed.filters, board, [], { mode: 'page', signal });
      if (signal?.aborted || !mutationLock.active) return { status: 'conflict' };
      if (validation.status !== 'ok') return { status: 'invalid' };
    }
    if (Object.hasOwn(next, 'catalog-filters')) {
      // Syntax-check every active catalog rule, including rules scoped to other
      // boards. Preserve the reviewed scope in storage; only this empty-card
      // validation packet uses the editor's all-board scope.
      const parsed = readCatalogFilters(next['catalog-filters']);
      const validation = await catalogMatcher.match(parsed.rules.map(rule => ({ ...rule, boards: '' })), board, [], { signal });
      if (signal?.aborted || !mutationLock.active) return { status: 'conflict' };
      if (validation.status !== 'ok') return { status: 'invalid' };
    }
    return locked(() => {
      if (signal?.aborted) return { status: 'conflict' };
      if (!hasLocks || !persistent || volatileSettings || volatileFilters || volatileCSS) return { status: 'unavailable' };
      try {
        if (keys.some(key => localStorage.getItem(key) !== previous[key])) return { status: 'conflict' };
      } catch { return { status: 'unavailable' }; }
      // Write enabling preferences last. Web Storage has no multi-key transaction;
      // cooperating writers share this lock, and failed writes are rolled back.
      const ordered = [...keys.filter(key => key !== settingsKey), settingsKey];
      const written = [];
      try {
        for (const key of ordered) {
          localStorage.setItem(key, next[key]);
          written.push(key);
        }
      } catch {
        for (const key of written.reverse()) {
          try {
            // Never undo a value replaced by a nonparticipating writer.
            if (localStorage.getItem(key) !== next[key]) continue;
            if (previous[key] === null) localStorage.removeItem(key);
            else localStorage.setItem(key, previous[key]);
          } catch { /* Report incomplete recovery below. */ }
        }
        let partial = true;
        try { partial = keys.some(key => localStorage.getItem(key) !== previous[key]); } catch { /* Storage is unavailable. */ }
        return { status: 'storage-error', partial };
      }
      // Invalidate before releasing the shared lock. A queued first-open save
      // must not add defaults to a deliberately sparse reviewed restore.
      if (settingsStartupState) settingsStartupState.active = false;
      return { status: 'ok', persisted: true };
    }, signal);
  }
  async function saveSettings(changes, signal, tabOnly = false) {
    let httpsFailed = false;
    const applied = await locked(() => {
      if (signal?.aborted) return false;
      const settings = { ...configuration(), ...changes };
      if (catalog && changes.threadWatcher === true) settings.disableAll = false;
      if (tabOnly) volatileSettings = true;
      if (!writeSettings(settings)) return false;
      if (httpsOrigin && Object.hasOwn(changes, 'forceHTTPS') && !volatileSettings) {
        try { document.cookie = httpsPreferenceCookie(settings.forceHTTPS); } catch { /* Verify the write below. */ }
        httpsFailed = httpsPreferenceEnabled(JSON.stringify(settings), readHTTPSCookie()) !== (settings.forceHTTPS === true);
      }
      refresh.cancel();
      enabled = settings.threadWatcher === true && settings.disableAll !== true;
      collapsed = mobile.matches;
      render();
      return true;
    }, signal);
    if (applied === false || signal?.aborted || !mutationLock.active) return false;
    if (enabled) { await acknowledgeCurrent(signal); if (signal?.aborted || !mutationLock.active) return false; navigateReadPosition(); }
    void nativeFilters?.refresh();
    return { persisted: !volatileSettings, httpsFailed,
      redirect: !volatileSettings && !httpsFailed
        ? settingsHTTPSRedirect(httpsOrigin, location.href.split('#', 1)[0], JSON.stringify(settingsCache), readHTTPSCookie()) : null };
  }

  function textWithBreaks(element) {
    try { return projection.text(element, ' '); } catch { return ''; }
  }
  function sections() {
    // The source text catalog has no watch buttons on its rows.
    if (catalog && document.getElementById('threads')?.dataset.textOnly === 'true') return [];
    return [...document.querySelectorAll('.board > .thread, #threads > .thread'),
      ...document.getElementById('catalogFiltered')?.content.querySelectorAll('.thread') || []];
  }
  function sectionId(section) { return postId(section.dataset.threadId || section.id.replace(/^t/, '')); }
  function posts(section) {
    return projection.queryAll(section, '.post[id]').map(post => postId(post.id.slice(1))).filter(Boolean);
  }
  function latest(section) {
    return catalog ? postId(section.dataset.latestReply) || sectionId(section)
      : posts(section).at(-1) || sectionId(section);
  }
  function label(section) {
    const teaser = section.querySelector('.teaser') || section.querySelector('template.catalogTeaser')?.content.querySelector('.teaser');
    const subject = catalog ? teaser?.querySelector('b')?.textContent : projection.query(section, '.op > .postInfo .subject')?.textContent;
    return watchLabel(subject, textWithBreaks(catalog ? teaser : projection.query(section, '.op .postMessage')), sectionId(section));
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
        for (const post of projection.queryAll(section, '.post[id]')) {
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
    if (menu.post.closest('.deleted') || menu.post.querySelector(':scope > .file.deleted')) return;
    const source = projection.query(menu.post, '.file > .fileText > a[href],.file > p > a[href]');
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
      const url = new URL(endpoint); url.searchParams.set(name === 'Yandex' ? 'img_url' : 'url', file.href);
      if (name === 'Yandex') url.searchParams.set('rpt', 'imageview');
      return [name, url.href];
    });
    if (sourceMobileLayout(mobile.matches, readNeverMobile())) {
      if (nativeDeletion?.canDelete(menu.post, true)) {
        postMenuItem(menu, 'del-file', 'Delete file', () => { void nativeDeletion.remove(menu.post, true); });
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
  const reportRegistry = createReportRegistry({ origin: location.origin,
    current: ({ board: expectedBoard, id, target }) => !catalog && mutationLock.active
      && configuration().disableAll !== true && expectedBoard === board
      && target.isConnected && document.getElementById(`p${id}`) === target
      && target.closest('.board') === document.querySelector('.board')
      && sections().includes(target.closest('.thread')),
    complete: ({ id, target }) => {
      if (target.classList.contains('op')) {
        if (!threadId && nativeThreads?.enabled()) void nativeThreads.hide?.(id);
      } else if (target.classList.contains('reply')) void nativeReplies?.hide?.(id);
    },
  });
  const receiveReport = event => { reportRegistry.receive(event); };
  window.addEventListener('message', receiveReport);
  function openReport(post) {
    const id = post.id.slice(1), url = reportURL(location.origin, board, id);
    if (!url || !post.isConnected || configuration().disableAll === true) return;
    let popup = null;
    try {
      popup = window.open(url, `report-popup-${board}-${id}-${Date.now()}-${Math.random().toString(36).slice(2)}`,
        'popup,toolbar=0,scrollbars=1,location=0,status=1,menubar=0,resizable=1,width=380,height=510');
    } catch { /* Popup blocking falls back to the same canonical native GET. */ }
    if (popup) reportRegistry.register(popup, board, id, post);
    else location.assign(url);
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
    postMenuItem(menu, 'report', 'Report post', () => openReport(post));
    if (!catalog && !threadId && post.classList.contains('op') && nativeThreads?.enabled()) {
      menu.hideThread = postMenuItem(menu, 'hide', '', () => { void nativeThreads.toggle(post.id.slice(1)); });
    }
    if (post.classList.contains('reply')) {
      menu.hide = postMenuItem(menu, 'hide-r', '', () => { void nativeReplies?.toggle(post.id.slice(1)); });
    }
    if (nativeDeletion?.canDelete(post)) postMenuItem(menu, 'del-post', 'Delete post', () => { void nativeDeletion.remove(post); });
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
      for (const post of projection.queryAll(section, '.post[id]')) {
        const id = postId(post.id.slice(1));
        const mobileLayout = sourceMobileLayout(mobile.matches, readNeverMobile());
        const info = post.querySelector(mobileLayout ? ':scope > .postInfoM' : ':scope > .postInfo');
        if (!id || !info) continue;
        let trigger = post.querySelector(':scope > .postInfo > [data-post-menu],:scope > .postInfoM > [data-post-menu]');
        if (!trigger && !disabled) {
          trigger = button('', event => {
            event.stopPropagation();
            openPostMenu(trigger, post, section, event.detail === 0 ? 'first' : null);
          }, 'postMenuBtn');
          trigger.dataset.postMenu = id;
          trigger.dataset.cmd = 'post-menu';
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
        trigger.dataset.family = themeFamily();
        trigger.hidden = disabled;
        trigger.textContent = mobileLayout ? '...' : '\u25b6';
        if (mobileLayout) {
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
      tracking.tracked(watchKey(board, threadId)), configuration().disableAll !== true, { ...nativeBacklinks, projection });
    nativeBacklinks?.refresh();
    nativeInlineQuotes?.refresh();
    nativeQuotePreview?.refresh();
    nativeImages?.refresh();
    nativeDeletion?.refresh();
    nativeEmbeds?.refresh();
    nativeCustomCSS?.refresh();
    nativeDisplay?.refresh();
    nativePosterIds?.refresh();
    nativePosterIdActions?.refresh();
    nativeLayout?.refresh();
    nativeFilters?.syncHeaders();
    nativeNavigation?.refresh();
    nativeExpansion?.refresh();
    nativeDepager?.refresh();
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
  const refreshNativePreferences = () => {
    if (!mutationLock.active) return;
    refresh.cancel();
    const settings = configuration();
    enabled = settings.threadWatcher === true && settings.disableAll !== true;
    collapsed = mobile.matches;
    render();
    void nativeFilters?.refresh();
  };
  document.addEventListener('4chanPreferencesRestored', refreshNativePreferences);
  document.addEventListener('4chanCatalogSettingsSaved', refreshNativePreferences);
  window.addEventListener('storage', event => {
    if (event.key === '4chan_never_show_mobile') { closePostMenu(); render(); return; }
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
  window.addEventListener('pagehide', () => { mutationLock.suspend(); closePostMenu(); refresh.cancel(); reportRegistry.clear(); window.removeEventListener('message', receiveReport); });
  window.addEventListener('pageshow', event => { if (event.persisted) { mutationLock.resume(); window.addEventListener('message', receiveReport); if (!catalog && !parsingBootstrap.ready()) resumeInitial(); } });
  mobile.addEventListener('change', () => { closePostMenu(); collapsed = mobile.matches; render(); });
  const container = document.getElementById('threads');
  if (container) new MutationObserver(controls).observe(container, { childList: true });
  render();
  const prepareInitial = async signal => {
    await tracking.consume(threadId);
    if (signal.aborted || !mutationLock.active) return false;
    await acknowledgeCurrent();
    if (signal.aborted || !mutationLock.active) return false;
    render();
    return true;
  };
  function resumeInitial() {
    if (initialSuspended) return;
    if (initialParsing.signal.aborted) initialParsing = new AbortController();
    const signal = initialParsing.signal;
    const initialReady = catalog ? prepareInitial(signal) : parsingBootstrap.run({
      sections: sections(), signal, prepare: prepareInitial,
      active: () => mutationLock.active && configuration().disableAll !== true,
      settle: async signal => {
        await new Promise(resolve => queueMicrotask(resolve));
        return nativeFilters ? nativeFilters.refreshSettled(signal) : true;
      },
    });
    void initialReady.then(ready => {
      if (!ready || signal.aborted || !mutationLock.active) return;
      render();
      navigateReadPosition(); return refreshAll(true);
    });
  }
  resumeInitial();
}
