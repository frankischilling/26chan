import { readCatalogTheme, writeCatalogTheme, catalogDropDownEnabled, CATALOG_THEME_LIMITS } from './native-settings.v1.js';
import { NativeWatchLock } from './native-filter.v1.js';

const themeKey = 'catalog-theme', settingsKey = '4chan-settings';
const controllers = new WeakMap();
export function updateCatalogSpoilers(root, enabled) {
  return controllers.get(root)?.setSpoilers(enabled) ?? Promise.resolve({ status: 'unavailable' });
}
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
function readSettings(raw) {
  if (raw === null) return {};
  if (typeof raw !== 'string' || raw.length > 4096) return null;
  try {
    const value = JSON.parse(raw);
    return record(value) && !Object.keys(value).some(key => ['__proto__', 'prototype', 'constructor'].includes(key)) ? value : null;
  } catch { return null; }
}
function node(tag, text, className) {
  const element = document.createElement(tag);
  if (text !== undefined) element.textContent = text;
  if (className) element.className = className;
  return element;
}
function button(text, action, className) {
  const element = node('button', text, className); element.type = 'button';
  element.addEventListener('click', action); return element;
}

export function mountCatalogTheme({ root, configuration, saveVolatileSettings, mobileLayout }) {
  if (!root?.isConnected || root.id !== 'watcher-context' || root.dataset.catalog !== 'true'
    || typeof configuration !== 'function' || typeof saveVolatileSettings !== 'function') return null;
  const form = document.getElementById('ctrl'), container = document.getElementById('threads');
  if (!form?.classList.contains('nativeCatalogControls') || !container?.isConnected) return null;
  let persistent = typeof navigator.locks?.request === 'function', themeRaw = null, settingsRaw = null;
  let dialog, fields, css, message, submit, opener, pending, spoilerPending, expected, expectedFlags, retired = false, suspended = false, lastCSS = '';
  let sheet = null;
  try { if (Array.isArray(document.adoptedStyleSheets)) sheet = new CSSStyleSheet(); } catch { /* Safe CSS application remains unavailable. */ }
  const live = () => !retired && !suspended && root.isConnected && form.isConnected && container.isConnected
    && document.getElementById('watcher-context') === root && document.getElementById('ctrl') === form
    && document.getElementById('threads') === container;
  const storage = () => {
    if (persistent) {
      try { themeRaw = localStorage.getItem(themeKey); settingsRaw = localStorage.getItem(settingsKey); }
      catch { persistent = false; }
    }
    return { theme: themeRaw, settings: settingsRaw };
  };
  // Existing values remain readable when the browser has no cross-tab lock.
  try { themeRaw = localStorage.getItem(themeKey); settingsRaw = localStorage.getItem(settingsKey); }
  catch { persistent = false; }
  const removeSheet = () => {
    if (sheet && document.adoptedStyleSheets.includes(sheet)) document.adoptedStyleSheets = document.adoptedStyleSheets.filter(value => value !== sheet);
  };
  const notify = (initial = false) => {
    if (!live()) return;
    const parsed = readCatalogTheme(storage().theme);
    removeSheet();
    if (parsed.status === 'ok' && parsed.styles?.rules.length && sheet) {
      try { sheet.replaceSync(parsed.styles.css); document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet]; }
      catch { removeSheet(); }
    }
    document.dispatchEvent(new CustomEvent('4chanCatalogThemeApplied', { detail: { raw: themeRaw, initial, persistent } }));
  };
  const lock = new NativeWatchLock({
    acquire: persistent ? async (action, signal) => {
      let entered = false;
      try { return await navigator.locks.request('paperboard-thread-watcher', { signal }, () => { entered = true; return action(); }); }
      catch (error) {
        if (entered || signal.aborted || error?.name !== 'SecurityError') throw error;
        persistent = false; return { status: 'unavailable' };
      }
    } : null,
    warn: () => { if (dialog?.open) message.textContent = 'Browser settings storage is busy or unavailable.'; },
  });
  const close = () => {
    pending?.abort(); pending = null;
    if (css) lastCSS = css.value;
    if (dialog?.open) dialog.close(); dialog?.classList.add('hidden');
    if (live() && opener?.isConnected) opener.focus();
  };
  async function setSpoilers(enabled) {
    if (typeof enabled !== 'boolean' || !live()) return { status: 'cancelled' };
    spoilerPending?.abort(); const request = new AbortController(); spoilerPending = request;
    const alive = () => live() && spoilerPending === request && !request.signal.aborted;
    const changed = raw => {
      const checked = readCatalogTheme(raw);
      if (checked.status !== 'ok') return { status: 'invalid' };
      const theme = { ...checked.theme };
      if (enabled) theme.nospoiler = true; else delete theme.nospoiler;
      const next = Object.keys(theme).length ? JSON.stringify(theme) : null;
      if (next !== null && next.length > CATALOG_THEME_LIMITS.storage) return { status: 'invalid' };
      return { status: 'ok', raw: next };
    };
    try {
      let result = persistent ? await lock.run(() => {
        if (!alive()) return { status: 'cancelled' };
        try {
          const next = changed(localStorage.getItem(themeKey));
          if (next.status !== 'ok') return next;
          if (next.raw === null) localStorage.removeItem(themeKey); else localStorage.setItem(themeKey, next.raw);
          themeRaw = next.raw; return { status: 'ok', persisted: true };
        } catch { persistent = false; return { status: 'unavailable' }; }
      }, request.signal) : { status: 'unavailable' };
      if (!alive()) return { status: 'cancelled' };
      if (result?.status === 'unavailable' && !persistent) {
        const next = changed(themeRaw);
        if (next.status !== 'ok') return next;
        themeRaw = next.raw; result = { status: 'ok', persisted: false };
      }
      if (result?.status === 'ok') notify();
      return result ?? { status: 'unavailable' };
    } catch { return { status: 'unavailable' }; }
    finally { if (spoilerPending === request) spoilerPending = null; }
  }
  async function save() {
    if (!live() || !dialog.open || pending) return;
    const theme = Object.fromEntries(['nobinds', 'nospoiler', 'newtab'].map(key => [key, fields.get(key).checked]));
    theme.css = css.value;
    const checked = writeCatalogTheme(theme);
    if (checked.status !== 'ok') { message.textContent = checked.error ?? 'Catalog settings are invalid.'; return; }
    const changes = { threadWatcher: fields.get('tw').checked, dropDownNav: fields.get('ddn').checked };
    const base = readSettings(expected.settings);
    if (base === null) { message.textContent = 'Stored native settings are invalid. Catalog settings were not saved.'; return; }
    const next = { ...base, ...changes };
    const enabling = changes.threadWatcher && !expectedFlags.threadWatcher || changes.dropDownNav && !expectedFlags.dropDownNav;
    if (enabling) next.disableAll = false;
    const nextRaw = JSON.stringify(next);
    if (nextRaw.length > 4096) { message.textContent = 'Native settings exceed their storage limit.'; return; }
    spoilerPending?.abort();
    const request = new AbortController(); pending = request; submit.disabled = true;
    const alive = () => live() && dialog.open && pending === request && !request.signal.aborted;
    try {
      let result = persistent ? await lock.run(() => {
        if (!alive()) return { status: 'cancelled' };
        try {
          if (localStorage.getItem(themeKey) !== expected.theme || localStorage.getItem(settingsKey) !== expected.settings) return { status: 'conflict' };
        } catch { persistent = false; return { status: 'unavailable' }; }
        const before = { [themeKey]: expected.theme, [settingsKey]: expected.settings };
        const values = { [themeKey]: checked.raw, [settingsKey]: nextRaw }, written = [];
        try {
          for (const key of [themeKey, settingsKey]) {
            if (values[key] === null) localStorage.removeItem(key); else localStorage.setItem(key, values[key]);
            written.push(key);
          }
        } catch {
          for (const key of written.reverse()) {
            try {
              if (localStorage.getItem(key) !== values[key]) continue;
              if (before[key] === null) localStorage.removeItem(key); else localStorage.setItem(key, before[key]);
            } catch { /* Check the complete recovery below. */ }
          }
          let partial = true;
          try { partial = [themeKey, settingsKey].some(key => localStorage.getItem(key) !== before[key]); } catch { /* Storage remains unavailable. */ }
          if (!partial) { persistent = false; return { status: 'unavailable' }; }
          return { status: 'storage-error', partial };
        }
        themeRaw = checked.raw; settingsRaw = nextRaw;
        return { status: 'ok' };
      }, request.signal) : { status: 'unavailable' };
      if (!alive()) return;
      if (result?.status === 'unavailable' && !persistent) {
        const applied = await saveVolatileSettings({ ...changes, ...(enabling ? { disableAll: false } : {}) }, request.signal);
        if (!alive()) return;
        if (applied === false || applied.persisted) { message.textContent = 'Settings could not be saved safely in this tab.'; return; }
        themeRaw = checked.raw; settingsRaw = nextRaw; expected = { theme: themeRaw, settings: settingsRaw };
        notify(); document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
        message.textContent = 'Settings are saved only in this tab. Browser storage or cross-tab locking is unavailable.';
      } else if (result?.status === 'ok') {
        notify(); document.dispatchEvent(new CustomEvent('4chanCatalogSettingsSaved'));
        document.dispatchEvent(new CustomEvent('4chanSettingsSaved')); close();
      } else if (result?.status === 'conflict') message.textContent = 'Settings changed in another tab. Reopen this editor before saving.';
      else if (result?.status === 'storage-error') {
        message.textContent = result.partial ? 'Saving failed and some previous values could not be recovered.' : 'Saving failed. Previous settings were recovered.';
        notify(); document.dispatchEvent(new CustomEvent('4chanCatalogSettingsSaved')); document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
      } else message.textContent = 'Settings could not be saved.';
    } catch { if (alive()) message.textContent = 'Settings could not be saved.'; }
    finally { if (pending === request) pending = null; submit.disabled = false; }
  }
  function build() {
    dialog = node('dialog', undefined, 'nativeSettings catalogSettings panel hidden'); dialog.id = 'theme';
    dialog.setAttribute('aria-label', 'Settings');
    const header = node('div', 'Settings', 'panelHeader');
    const dismiss = button('', close, 'icon closeIcon'); dismiss.id = 'theme-close'; dismiss.setAttribute('aria-label', 'Close settings'); header.append(dismiss);
    dialog.append(header, node('h4', 'Options'));
    fields = new Map(); const list = node('ul', undefined, 'clickset');
    for (const [key, label, desktop] of [['nobinds', 'Disable keybinds', true], ['nospoiler', "Don't spoiler images", false],
      ['newtab', 'Open threads in a new tab', false], ['tw', 'Thread Watcher', true], ['ddn', 'Use drop-down navigation', true]]) {
      const row = node('li', undefined, desktop ? 'desktop' : ''), caption = node('label'), input = node('input', undefined, 'menuOption');
      input.type = 'checkbox'; input.id = `theme-${key}`;
      input.dataset.option = ({ tw: 'threadWatcher', ddn: 'dropDownNav' })[key] ?? key;
      caption.append(input, ` ${label}`); row.append(caption); list.append(row); fields.set(key, input);
    }
    dialog.append(list, node('h4', 'Shortcuts', 'desktop'));
    const shortcuts = node('ul', undefined, 'clickset desktop');
    for (const [key, text] of [['R', 'Refresh current page'], ['X', 'Reorder threads'], ['S', 'Open search box, Esc to close'],
      ['Shift LMB', 'Hide threads'], ['Alt LMB', 'Pin threads'], ['RMB', 'Threads context menu']]) {
      const row = node('li'); row.append(node('kbd', key), ` — ${text}`); shortcuts.append(row);
    }
    dialog.append(shortcuts, node('h4', 'Custom CSS'));
    css = node('textarea'); css.id = 'theme-css'; css.rows = 4; css.cols = 45; css.maxLength = CATALOG_THEME_LIMITS.css;
    css.setAttribute('aria-label', 'Catalog Custom CSS'); dialog.append(css);
    const actions = node('div'); actions.id = 'theme-btns'; message = node('span'); message.id = 'theme-msg'; message.setAttribute('role', 'status');
    const center = node('div', undefined, 'center'); submit = button('Save Settings', save); submit.id = 'theme-save'; center.append(submit);
    actions.append(message, center); dialog.append(actions); document.body.append(dialog);
    dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
  }
  function open(source) {
    if (!live()) return;
    if (!dialog) build();
    if (dialog.open) { close(); return; }
    spoilerPending?.abort(); opener = source; expected = storage(); const parsed = readCatalogTheme(expected.theme);
    const theme = parsed.status === 'ok' ? parsed.theme : {}, settings = configuration();
    for (const key of ['nobinds', 'nospoiler', 'newtab']) fields.get(key).checked = theme[key] === true;
    fields.get('tw').checked = settings.disableAll !== true && settings.threadWatcher === true;
    fields.get('ddn').checked = catalogDropDownEnabled(expected.settings, mobileLayout());
    expectedFlags = { threadWatcher: fields.get('tw').checked, dropDownNav: fields.get('ddn').checked };
    css.value = theme.css || lastCSS; message.textContent = parsed.cssError ? 'Stored Custom CSS could not be applied. Review it before saving.'
      : parsed.status !== 'ok' ? 'Stored catalog settings are invalid. Review these values before replacing them.' : '';
    dialog.style.top = `${scrollY + 60}px`; dialog.classList.remove('hidden'); dialog.showModal();
    fields.get(mobileLayout() ? 'nospoiler' : 'nobinds').focus();
    document.dispatchEvent(new CustomEvent('4chanCatalogThemeEditorReady'));
  }
  window.addEventListener('storage', event => {
    if (!persistent || event.key !== null && ![themeKey, settingsKey].includes(event.key)) return;
    if (pending) { pending.abort(); if (dialog?.open) message.textContent = 'Settings changed in another tab. Reopen this editor before saving.'; }
    spoilerPending?.abort();
    notify();
  });
  window.addEventListener('pagehide', () => { suspended = true; spoilerPending?.abort(); close(); removeSheet(); });
  window.addEventListener('pageshow', event => { if (event.persisted && !retired) { suspended = false; notify(); } });
  const observer = new MutationObserver(() => {
    if (!root.isConnected || !form.isConnected || !container.isConnected || document.getElementById('watcher-context') !== root
      || document.getElementById('ctrl') !== form || document.getElementById('threads') !== container) {
      retired = true; spoilerPending?.abort(); close(); removeSheet(); observer.disconnect();
    }
  });
  observer.observe(document.body, { childList: true, subtree: true }); notify(true);
  const controller = { open, refresh: notify, setSpoilers }; controllers.set(root, controller);
  return controller;
}
