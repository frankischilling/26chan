import { readWatcherPosition, writeWatcherPosition, dragWatcherPosition } from './watcher-position.v1.js';
import { customBoards } from './native-display.v1.js';
import { publicBoardPath } from './page-chrome.v1.js';

export const NAVIGATION_LIMITS = Object.freeze({ boards: 100, bytes: 32768, reads: 4096, requestMs: 5000 });
const slug = value => typeof value === 'string' && /^[a-z0-9]{1,10}$/.test(value);
const navigationMounts = new WeakMap();
const sameObject = (value, keys) => value && typeof value === 'object' && !Array.isArray(value)
  && Object.keys(value).sort().join(',') === [...keys].sort().join(',');

export function navigationPage(path, board) {
  if (!slug(board) || typeof path !== 'string') return null;
  if (path === `/${board}/`) return 0;
  const match = path.match(/^\/[a-z0-9]{1,10}\/(0|[1-9][0-9]{0,2})$/);
  return match && path === `/${board}/${match[1]}` ? Number(match[1]) : null;
}

export function parseNavigationDirectory(raw) {
  if (typeof raw !== 'string' || raw.length > NAVIGATION_LIMITS.bytes
    || new TextEncoder().encode(raw).length > NAVIGATION_LIMITS.bytes) throw new TypeError('navigation-size');
  const value = JSON.parse(raw);
  if (!sameObject(value, ['version', 'boards']) || value.version !== 1
    || !Array.isArray(value.boards) || value.boards.length > NAVIGATION_LIMITS.boards) throw new TypeError('navigation-directory');
  const seen = new Set(), encoder = new TextEncoder();
  for (const entry of value.boards) {
    if (!sameObject(entry, ['board', 'title']) || !slug(entry.board) || seen.has(entry.board)
      || typeof entry.title !== 'string' || !entry.title || encoder.encode(entry.title).length > 120) throw new TypeError('navigation-board');
    seen.add(entry.board);
  }
  return value.boards;
}

function serverNavigationDirectory(root, window) {
  if (root !== window.document.body || !root.classList.contains('publicPageChrome')) return null;
  const select = root.querySelector('#boardSelectMobile');
  if (!(select instanceof window.HTMLSelectElement) || !select.options.length || select.options.length > NAVIGATION_LIMITS.boards) return null;
  try {
    const options = [...select.options];
    const boards = options.map(option => {
      const prefix = `/${option.value}/ - `;
      if (!option.textContent.startsWith(prefix)) throw new TypeError('navigation-label');
      return { board: option.value, title: option.textContent.slice(prefix.length) };
    });
    return { boards: parseNavigationDirectory(JSON.stringify({ version: 1, boards })),
      nws: new Set(options.filter(option => option.classList.contains('nwsb')).map(option => option.value)) };
  } catch { return null; }
}

function cancelBody(body) { try { body?.cancel()?.catch(() => {}); } catch { /* Already closed. */ } }

export function navigationDirectory({ origin, fetcher = globalThis.fetch?.bind(globalThis), signal, requestMs = 5000 }) {
  let base;
  try { base = new URL(origin); } catch { return Promise.resolve({ status: 'invalid-context' }); }
  if (!['http:', 'https:'].includes(base.protocol) || base.origin !== origin || base.username || base.password
    || !Number.isInteger(requestMs) || requestMs < 1 || requestMs > NAVIGATION_LIMITS.requestMs) return Promise.resolve({ status: 'invalid-context' });
  if (signal?.aborted) return Promise.resolve({ status: 'cancelled' });
  if (typeof fetcher !== 'function') return Promise.resolve({ status: 'unavailable' });
  const url = `${origin}/_watch/boards`, controller = new AbortController();
  return new Promise(resolve => {
    let done = false, reader, body, timer;
    const finish = result => {
      if (done) return;
      done = true; clearTimeout(timer); signal?.removeEventListener('abort', cancelled);
      controller.abort(); cancelBody(reader ?? body); resolve(result);
    };
    const cancelled = () => finish({ status: 'cancelled' });
    signal?.addEventListener('abort', cancelled, { once: true });
    timer = setTimeout(() => finish({ status: 'timeout' }), requestMs);
    void (async () => {
      const response = await fetcher(url, { method: 'GET', credentials: 'omit', mode: 'same-origin', redirect: 'error',
        cache: 'no-store', headers: { Accept: 'application/json' }, signal: controller.signal });
      if (done) { cancelBody(response.body); return; }
      body = response.body;
      if (response.redirected || response.url !== url || response.status !== 200
        || response.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/json' || !body) {
        finish({ status: 'invalid-response' }); return;
      }
      const length = response.headers.get('content-length');
      if (length !== null && (!/^\d+$/.test(length) || Number(length) > NAVIGATION_LIMITS.bytes)) { finish({ status: 'response-limit' }); return; }
      reader = body.getReader();
      const chunks = []; let bytes = 0, reads = 0;
      for (;;) {
        const next = await reader.read();
        if (done) return;
        if (next.done) break;
        if (!(next.value instanceof Uint8Array)) { finish({ status: 'invalid-response' }); return; }
        bytes += next.value.length;
        if (bytes > NAVIGATION_LIMITS.bytes || ++reads > NAVIGATION_LIMITS.reads) { finish({ status: 'response-limit' }); return; }
        chunks.push(next.value);
      }
      const all = new Uint8Array(bytes); let offset = 0;
      for (const chunk of chunks) { all.set(chunk, offset); offset += chunk.length; }
      const raw = new TextDecoder('utf-8', { fatal: true }).decode(all);
      finish({ status: 'ok', boards: parseNavigationDirectory(raw) });
    })().catch(() => finish({ status: 'unavailable' }));
  });
}

function mountNavigationPosition({ panel, handle, key, fixed, settings, save, enabled, offsetTop = () => 0, window }) {
  let drag = null, saving = false, retired = false, savingController = null, suppressClick = false;
  const initial = key === 'SN-position' ? { right: '10px', top: '50px' } : { left: '10px', top: '50px' };
  const raw = () => typeof settings()[key] === 'string' ? settings()[key] : null;
  const read = value => {
    const parsed = readWatcherPosition(value) ?? {};
    return { ...(parsed.left || parsed.right ? (parsed.left ? { left: parsed.left } : { right: parsed.right })
      : (initial.left ? { left: initial.left } : { right: initial.right })),
    ...(parsed.top || parsed.bottom ? (parsed.top ? { top: parsed.top } : { bottom: parsed.bottom }) : { top: initial.top }) };
  };
  let position = read(raw()), stored = raw();
  const minimumTop = (height, panelHeight) => {
    let value = 0;
    try { value = Number(offsetTop()); } catch { /* Use the page edge. */ }
    const maximum = Math.max(0, height - panelHeight);
    return Number.isFinite(value) ? Math.max(0, Math.min(value, maximum)) : 0;
  };
  const apply = () => {
    const width = window.document.documentElement.clientWidth, height = window.document.documentElement.clientHeight;
    const pixels = (value, span) => parseFloat(value) * (value.endsWith('%') ? span / 100 : 1);
    const x = position.left ? pixels(position.left, width) : width - pixels(position.right, width) - panel.offsetWidth;
    const y = position.top ? pixels(position.top, height) : height - pixels(position.bottom, height) - panel.offsetHeight;
    panel.style.right = panel.style.bottom = '';
    panel.style.left = `${Math.max(0, Math.min(x, width - panel.offsetWidth))}px`;
    const maxHeight = fixed ? height : Math.max(height, window.document.documentElement.scrollHeight);
    panel.style.top = `${Math.max(minimumTop(height, panel.offsetHeight), Math.min(y, maxHeight - panel.offsetHeight))}px`;
  };
  const cancel = () => {
    if (!drag) return;
    const old = drag; drag = null; position = old.position;
    try { if (handle.hasPointerCapture(old.id)) handle.releasePointerCapture(old.id); } catch { /* Detached handle. */ }
    apply();
  };
  const sync = () => {
    const next = raw();
    if (next !== stored || !enabled() || (drag && (drag.geometry.width !== window.document.documentElement.clientWidth
      || drag.geometry.height !== window.document.documentElement.clientHeight))) { cancel(); savingController?.abort(); }
    if (!drag) { stored = next; position = read(next); apply(); }
  };
  const geometry = (rect, dx = 0, dy = 0) => ({ width: window.document.documentElement.clientWidth,
    height: window.document.documentElement.clientHeight, panelWidth: rect.width, panelHeight: rect.height,
    dx, dy, scrollX: fixed ? 0 : window.scrollX, scrollY: fixed ? 0 : window.scrollY,
    offsetTop: minimumTop(window.document.documentElement.clientHeight, rect.height) });
  async function commit(expected) {
    const value = writeWatcherPosition(position, fixed);
    if (!value || !enabled() || retired) return;
    saving = true; const controller = new AbortController(); savingController = controller;
    try { await save?.(key, value, expected, controller.signal); } catch { /* Keep the stored or volatile preference authoritative. */ }
    finally { controller.abort(); if (savingController === controller) savingController = null; saving = false; if (!retired) sync(); }
  }
  const down = event => {
    if (!event.shiftKey || event.button !== 0 || event.isPrimary === false || saving || !enabled()) return;
    sync(); const rect = panel.getBoundingClientRect();
    drag = { id: event.pointerId, position: { ...position }, expected: stored,
      geometry: geometry(rect, event.clientX - rect.left, event.clientY - rect.top) };
    handle.setPointerCapture(event.pointerId); event.preventDefault();
  };
  const move = event => {
    if (!drag || drag.id !== event.pointerId) return;
    const next = dragWatcherPosition(event.clientX, event.clientY, drag.geometry);
    if (next) { position = next; apply(); }
  };
  const up = event => {
    if (!drag || drag.id !== event.pointerId) return;
    const old = drag; drag = null; suppressClick = true;
    if (handle.hasPointerCapture(event.pointerId)) handle.releasePointerCapture(event.pointerId);
    void commit(old.expected);
  };
  const keydown = event => {
    if (!event.shiftKey || event.altKey || event.ctrlKey || event.metaKey || saving || !enabled()
      || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return;
    event.preventDefault(); sync(); const rect = panel.getBoundingClientRect();
    const next = dragWatcherPosition(rect.left + (event.key === 'ArrowLeft' ? -10 : event.key === 'ArrowRight' ? 10 : 0),
      rect.top + (event.key === 'ArrowUp' ? -10 : event.key === 'ArrowDown' ? 10 : 0), geometry(rect));
    if (next) { position = next; apply(); void commit(stored); }
  };
  const click = event => { if (suppressClick) { suppressClick = false; event.preventDefault(); event.stopImmediatePropagation(); } };
  handle.tabIndex = 0;
  handle.setAttribute('aria-label', key === 'SN-position' ? 'Move navigation arrows with Shift and arrow keys' : 'Move page navigation with Shift and arrow keys');
  panel.dataset.trackpos = key;
  for (const [name, callback] of [['pointerdown', down], ['pointermove', move], ['pointerup', up],
    ['pointercancel', cancel], ['lostpointercapture', cancel], ['keydown', keydown]]) handle.addEventListener(name, callback);
  handle.addEventListener('click', click, true);
  apply();
  return { sync, destroy() {
    retired = true; cancel(); savingController?.abort(); handle.removeEventListener('click', click, true);
    for (const [name, callback] of [['pointerdown', down], ['pointermove', move], ['pointerup', up],
      ['pointercancel', cancel], ['lostpointercapture', cancel], ['keydown', keydown]]) handle.removeEventListener(name, callback);
  } };
}

export function mountNativeNavigation({ root, board, thread, catalog = false, settings, savePosition,
  openSettings, openCustomMenu, mobile, readNeverMobile = () => null, fetcher, decorateButton }) {
  const document = root?.ownerDocument, window = document?.defaultView;
  if (!root || !window || !slug(board) || typeof settings !== 'function') return null;
  navigationMounts.get(root)?.destroy();
  const serverDirectory = serverNavigationDirectory(root, window);
  const positions = new Map(); let bar = null, top = null, arrows = null, directory = serverDirectory?.boards ?? null, pending = null;
  let signature = null, suspended = false, destroyed = false, hideTimer = null, previousScroll = window.scrollY;
  let autoHideActive = false, controller = null;
  let ownsDropDownClass = false;
  let resizeObserver = null, ownedOffset = null, priorOffset = null;
  const disabled = () => suspended || destroyed || !root.isConnected || root.ownerDocument !== document || settings().disableAll === true;
  const mobileLayout = () => mobile?.matches === true && readNeverMobile() !== 'true';
  const element = (tag, text, className) => {
    const result = document.createElement(tag); if (text !== undefined) result.textContent = text;
    if (className) result.className = className; return result;
  };
  function link(text, href) { const result = element('a', text); result.href = href; return result; }
  function button(text, action, title = text) {
    const result = element('button', text); result.type = 'button'; result.title = title;
    result.addEventListener('click', action); return result;
  }
  function clearPosition(key) { positions.get(key)?.destroy(); positions.delete(key); }
  function clear() {
    pending?.abort(); pending = null; window.clearTimeout(hideTimer); hideTimer = null;
    autoHideActive = false;
    resizeObserver?.disconnect(); resizeObserver = null;
    if (ownedOffset !== null && document.body.style.getPropertyValue('--native-navigation-height') === ownedOffset) {
      if (priorOffset?.value) document.body.style.setProperty('--native-navigation-height', priorOffset.value, priorOffset.priority);
      else document.body.style.removeProperty('--native-navigation-height');
    }
    ownedOffset = priorOffset = null;
    for (const key of positions.keys()) clearPosition(key);
    bar?.remove(); top?.remove(); arrows?.remove(); bar = top = arrows = null;
    if (ownsDropDownClass) document.body.classList.remove('hasDropDownNav'); ownsDropDownClass = false;
  }
  function boardLinks(config) {
    const chosen = config.customMenu === true ? customBoards(config.customMenuList) : null;
    return chosen?.length ? chosen.map(board => ({ board, title: directory?.find(row => row.board === board)?.title ?? board }))
      : directory ?? [{ board, title: `/${board}/` }];
  }
  function updateBoards() {
    if (!bar) return;
    const config = settings(), list = bar.querySelector('.nativeBoardLinks'), select = bar.querySelector('select');
    const entries = list ? boardLinks(config) : directory ?? [{ board, title: `/${board}/` }];
    if (list) {
      list.replaceChildren(document.createTextNode('[ '));
      entries.forEach((entry, index) => {
        if (index) list.append(' / ');
        const custom = config.customMenu === true && customBoards(config.customMenuList)?.length;
        const anchor = link(entry.board, publicBoardPath(entry.board, catalog && !custom)); anchor.title = entry.title; list.append(anchor);
      });
      list.append(' ]');
    }
    if (select) {
      select.replaceChildren();
      for (const entry of entries) {
        const option = element('option', `/${entry.board}/ - ${entry.title}`); option.value = entry.board;
        if (serverDirectory?.nws.has(entry.board)) option.className = 'nwsb';
        option.selected = entry.board === board; select.append(option);
      }
      if (!entries.some(entry => entry.board === board)) {
        const current = element('option', `/${board}/`); current.value = board; current.selected = true; select.prepend(current);
      }
      const custom = bar.querySelector('.nativeCustomBoardLinks');
      custom.replaceChildren();
      const chosen = config.customMenu === true ? customBoards(config.customMenuList) : null;
      custom.hidden = !chosen?.length;
      if (chosen?.length) {
        custom.append('[ ');
        chosen.forEach((board, index) => { if (index) custom.append(' / '); custom.append(link(board, publicBoardPath(board))); });
        custom.append(' ]');
      }
    }
  }
  async function loadBoards() {
    if (!bar || pending || directory || disabled() || window.navigator.onLine === false || document.hidden) return;
    const current = new AbortController(); pending = current;
    const result = await navigationDirectory({ origin: window.location.origin, fetcher, signal: current.signal });
    if (pending !== current || current.signal.aborted || disabled()) return;
    pending = null;
    if (result.status === 'ok') { directory = result.boards; updateBoards(); }
    else if (bar) bar.querySelector('.nativeNavigationStatus').textContent = 'Board list unavailable. Use All boards.';
  }
  function persistent(config) {
    bar = element('nav', undefined, 'nativePersistentNavigation'); bar.setAttribute('aria-label', 'Persistent board navigation');
    const classic = config.classicNav === true && !mobileLayout();
    if (classic) bar.append(element('span', undefined, 'nativeBoardLinks'));
    else {
      const label = element('label', 'Board '), select = element('select'); select.setAttribute('aria-label', 'Board');
      select.addEventListener('change', () => {
        if (disabled() || !bar?.isConnected || !bar.contains(select)) return;
        if (select.value !== board && !directory?.some(entry => entry.board === select.value)) return;
        const path = publicBoardPath(select.value, catalog); if (path) window.location.assign(path);
      });
      label.append(select); bar.append(label, element('span', undefined, 'nativeCustomBoardLinks'));
    }
    bar.append(' ', link('All boards', '/'), ' ', button('Settings', event => openSettings?.(event.currentTarget)),
      ' ', button('Edit boards', event => openCustomMenu?.(event.currentTarget)), ' ',
      button('Bottom', () => window.scrollTo(0, document.documentElement.scrollHeight)));
    const state = element('span', '', 'nativeNavigationStatus'); state.setAttribute('role', 'status'); bar.append(state);
    if (!document.body.classList.contains('hasDropDownNav')) { ownsDropDownClass = true; document.body.classList.add('hasDropDownNav'); }
    priorOffset = { value: document.body.style.getPropertyValue('--native-navigation-height'), priority: document.body.style.getPropertyPriority('--native-navigation-height') };
    const fit = () => {
      if (!bar || (ownedOffset !== null && document.body.style.getPropertyValue('--native-navigation-height') !== ownedOffset)) return;
      ownedOffset = `${Math.max(36, Math.min(1024, Math.ceil(bar.getBoundingClientRect().height))) + 8}px`;
      document.body.style.setProperty('--native-navigation-height', ownedOffset);
      for (const position of positions.values()) position.sync();
    };
    document.body.prepend(bar); updateBoards(); fit(); void loadBoards();
    if (typeof window.ResizeObserver === 'function') { resizeObserver = new window.ResizeObserver(fit); resizeObserver.observe(bar); }
    bar.addEventListener('focusin', () => {
      if (!bar) return;
      bar.style.top = '';
      previousScroll = window.scrollY;
    });
  }
  function dock(key, panel, handle, fixed) {
    document.body.append(panel);
    positions.set(key, mountNavigationPosition({ panel, handle, key, fixed, settings, save: savePosition,
      enabled: () => !disabled() && settings()[key === 'SN-position' ? 'stickyNav' : 'topPageNav'] === true,
      offsetTop: () => bar && settings().autoHideNav !== true ? bar.offsetHeight : 0,
      window }));
  }
  function pagination() {
    const current = navigationPage(window.location.pathname, board);
    const original = document.querySelector('nav.pages');
    if (current === null || !original || thread || catalog) return;
    top = element('nav', undefined, 'topPageNav nativeNavigationDock'); top.setAttribute('aria-label', 'Top page navigation');
    const handle = element('div');
    for (const relation of ['prev', 'next']) {
      if (relation === 'next') handle.append(` Page ${current + 1} `);
      const source = original.querySelector(`a[rel="${relation}"]`);
      if (source && navigationPage(source.getAttribute('href'), board) !== null) {
        const anchor = link(relation === 'prev' ? 'Previous' : 'Next', source.getAttribute('href')); anchor.rel = relation;
        handle.append(anchor);
      }
    }
    top.append(handle); dock('TN-position', top, handle, false);
  }
  function navigationArrows() {
    arrows = element('nav', undefined, 'nativeNavigationDock'); arrows.id = 'stickyNav'; arrows.setAttribute('aria-label', 'Page navigation arrows');
    const handle = element('div');
    for (const [description, glyph, name, end] of [['Top', '\u25b2', 'arrow_up', false], ['Bottom', '\u25bc', 'arrow_down', true]]) {
      const control = button(glyph, () => window.scrollTo(0, end ? document.documentElement.scrollHeight : 0), description);
      control.setAttribute('aria-label', description);
      if (decorateButton) { control.textContent = ''; decorateButton(control, name, description); }
      handle.append(control);
    }
    arrows.append(handle); dock('SN-position', arrows, handle, true);
  }
  function refresh() {
    const config = settings();
    const chosen = config.customMenu === true ? customBoards(config.customMenuList) : null;
    const next = JSON.stringify([disabled(), config.dropDownNav === true, config.classicNav === true, config.topPageNav === true,
      config.stickyNav === true, mobileLayout(), chosen]);
    if (next !== signature) {
      clear(); signature = next;
      if (!disabled()) {
        // Source Main.run only installs persistent navigation outside mobile
        // layout. Keep its stored preference for a later desktop layout.
        if (config.dropDownNav === true && (catalog || !mobileLayout())) persistent(config);
        if (config.topPageNav === true) pagination();
        if (config.stickyNav === true) navigationArrows();
      }
    }
    for (const position of positions.values()) position.sync();
    const nextAutoHide = !!bar && !disabled() && config.autoHideNav === true;
    if (nextAutoHide !== autoHideActive) {
      window.clearTimeout(hideTimer); hideTimer = null;
      if (bar) bar.style.top = '';
      previousScroll = window.scrollY;
      autoHideActive = nextAutoHide;
    } else if (!nextAutoHide && bar) {
      window.clearTimeout(hideTimer); hideTimer = null; bar.style.top = '';
    }
  }
  function themeChanged() {
    if (!arrows || !decorateButton) return;
    const controls = arrows.querySelectorAll('button');
    for (const [index, [description, name]] of [['Top', 'arrow_up'], ['Bottom', 'arrow_down']].entries()) {
      const control = controls[index];
      if (control) decorateButton(control, name, description);
    }
  }
  const scroll = () => {
    window.clearTimeout(hideTimer);
    hideTimer = null;
    if (!autoHideActive || disabled() || !bar || settings().autoHideNav !== true) return;
    hideTimer = window.setTimeout(() => {
      hideTimer = null;
      if (disabled() || !bar || settings().autoHideNav !== true || document.hidden) return;
      const position = window.scrollY;
      if (Math.abs(position - previousScroll) <= 5) return;
      const upward = position < previousScroll;
      previousScroll = position;
      if (bar.contains(document.activeElement)) { bar.style.top = ''; return; }
      bar.style.top = upward ? '' : `-${Math.min(bar.offsetHeight, 1024)}px`;
    }, 50);
  };
  const storage = event => { if (event.key === null || ['4chan-settings', '4chan_never_show_mobile'].includes(event.key)) refresh(); };
  const hide = () => { suspended = true; clear(); signature = null; };
  const show = () => { suspended = false; refresh(); };
  const visibility = () => {
    if (document.hidden || window.navigator.onLine === false) { pending?.abort(); pending = null; window.clearTimeout(hideTimer); }
    else void loadBoards();
  };
  document.addEventListener('4chanSettingsSaved', refresh); document.addEventListener('visibilitychange', visibility);
  window.addEventListener('storage', storage); window.addEventListener('scroll', scroll, { passive: true });
  window.addEventListener('resize', refresh); window.addEventListener('offline', visibility); window.addEventListener('online', visibility);
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show); mobile?.addEventListener('change', refresh);
  refresh();
  controller = { refresh, themeChanged, destroy() {
    if (destroyed) return; destroyed = true; clear();
    document.removeEventListener('4chanSettingsSaved', refresh); document.removeEventListener('visibilitychange', visibility);
    window.removeEventListener('storage', storage); window.removeEventListener('scroll', scroll); window.removeEventListener('resize', refresh);
    window.removeEventListener('offline', visibility); window.removeEventListener('online', visibility);
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show); mobile?.removeEventListener('change', refresh);
    if (navigationMounts.get(root) === controller) navigationMounts.delete(root);
  } };
  navigationMounts.set(root, controller);
  return controller;
}
