// Fixed public navigation; board data and destination paths stay local.
const pageMounts = new WeakMap();
export function publicBoardPath(board, catalog = false) {
  if (typeof board !== 'string' || !/^[a-z0-9]{1,10}$/.test(board) || typeof catalog !== 'boolean') return null;
  return `/${board}/${catalog && board !== 'f' ? 'catalog' : ''}`;
}

const announcementMounts = new WeakMap();
export function announcementDismissed(current, previous) {
  return typeof current === 'string' && /^[0-9]{1,20}$/.test(current)
    && typeof previous === 'string' && previous.length > 0 && previous.length <= 64
    && current <= previous;
}

// The source core mobile toggle works even when the native extension is off.
export function mountMobileAnnouncement(body) {
  const document = body?.ownerDocument, button = document?.getElementById('globalToggle');
  const message = document?.getElementById('globalMessage');
  if (!button || !message || !body.contains(button) || !body.contains(message)) return null;
  const click = event => {
    if (!button.isConnected || !message.isConnected || !body.contains(button) || !body.contains(message)) return;
    event.preventDefault();
    const opening = button.classList.contains('hidden');
    button.classList.toggle('hidden', !opening); button.classList.toggle('shown', opening);
    message.classList.toggle('hideMobile', !opening);
    button.textContent = opening ? 'Close Announcement' : 'View Announcement';
    button.setAttribute('aria-expanded', String(opening));
  };
  button.addEventListener('click', click);
  return { destroy() { button.removeEventListener('click', click); } };
}

export function mountDesktopAnnouncement(body, { settings, mobileLayout = false, catalog = false } = {}) {
  const document = body?.ownerDocument, window = document?.defaultView;
  const message = document?.getElementById('globalMessage');
  if (!window || !message || !body.contains(message) || !message.textContent
    || !/^[0-9]{1,20}$/.test(message.dataset.utc || '') || typeof settings !== 'function') return null;
  announcementMounts.get(body)?.destroy();
  const initialDisplay = message.style.display, sibling = message.nextElementSibling;
  const initialClear = sibling?.style.clear;
  let button = null, retired = false, suspended = false, ownedDisplay = null;
  const key = '4chan-global-msg';
  function show(hidden) {
    ownedDisplay = hidden ? 'none' : '';
    message.style.display = ownedDisplay;
    button.setAttribute('aria-expanded', String(!hidden));
    button.style.opacity = hidden ? '0.5' : '1';
    button.classList.toggle('expandIcon', hidden); button.classList.toggle('collapseIcon', !hidden);
  }
  function remove() {
    button?.remove(); button = null;
    if (ownedDisplay !== null && message.style.display === ownedDisplay) message.style.display = initialDisplay;
    ownedDisplay = null;
    if (sibling?.style.clear === 'both') sibling.style.clear = initialClear;
  }
  function refresh() {
    if (retired || suspended) return;
    if (!body.isConnected || !message.isConnected || !body.contains(message)) { destroy(); return; }
    let active = catalog;
    try { active ||= settings().disableAll !== true; } catch { active = false; }
    if (mobileLayout || !active) { remove(); return; }
    if (button) return;
    let previous = null;
    try { previous = window.localStorage.getItem(key); } catch {
      if (catalog) return; // Catalog's source initializer requires WebStorage.
    }
    button = document.createElement('button'); button.type = 'button'; button.id = 'toggleMsgBtn';
    button.className = 'globalMessageToggle extButton'; button.title = 'Toggle announcement';
    button.setAttribute('aria-label', 'Toggle announcement'); button.setAttribute('aria-controls', 'globalMessage');
    button.addEventListener('click', () => {
      if (retired || suspended || !button?.isConnected || !message.isConnected) return;
      const hidden = message.style.display !== 'none'; show(hidden);
      try {
        if (hidden) window.localStorage.setItem(key, message.dataset.utc);
        else window.localStorage.removeItem(key);
      } catch { /* Keep the toggle usable; persistence is optional. */ }
    });
    message.before(button); if (sibling) sibling.style.clear = 'both';
    show(announcementDismissed(message.dataset.utc, previous));
  }
  const hide = event => { if (event.persisted) suspended = true; else destroy(); };
  const showPage = event => { if (event.persisted && !retired) { suspended = false; refresh(); } };
  function destroy() {
    if (retired) return; retired = true; remove();
    document.removeEventListener('4chanSettingsSaved', refresh);
    document.removeEventListener('4chanPreferencesRestored', refresh);
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', showPage);
    if (announcementMounts.get(body) === api) announcementMounts.delete(body);
  }
  const api = { refresh, destroy }; announcementMounts.set(body, api);
  document.addEventListener('4chanSettingsSaved', refresh);
  document.addEventListener('4chanPreferencesRestored', refresh);
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', showPage);
  refresh(); return api;
}

export function mountPageChrome(body) {
  const document = body?.ownerDocument, window = document?.defaultView;
  if (!window || !(body instanceof window.HTMLBodyElement) || document.body !== body || !body.isConnected
    || !body.classList.contains('publicPageChrome') || !['true', 'false'].includes(body.dataset.pageCatalog)) return null;
  pageMounts.get(body)?.destroy();
  const announcement = mountMobileAnnouncement(body);
  const catalog = body.dataset.pageCatalog === 'true';
  const select = document.getElementById('boardSelectMobile');
  const choices = select instanceof window.HTMLSelectElement && select.options.length <= 100
    ? new Set([...select.options].map(option => option.value)) : new Set();
  const controls = [...body.querySelectorAll('[data-page-mobile="enable"], [data-page-mobile="disable"]')].slice(0, 6);
  const previous = body.getAttribute('data-native-never-mobile');
  const key = '4chan_never_show_mobile';
  let owned = null, suspended = false, retired = false, notice = null, api;
  const live = () => !retired && !suspended && body.isConnected && document.body === body;
  function refresh() {
    if (!live()) return;
    let disabled = false;
    try { disabled = window.localStorage.getItem(key) === 'true'; } catch { /* Keep the public mobile default. */ }
    owned = String(disabled); body.setAttribute('data-native-never-mobile', owned);
  }
  const change = () => {
    if (!live() || !select.isConnected || !body.contains(select) || !choices.has(select.value)) return;
    const path = publicBoardPath(select.value, catalog);
    if (path) window.location.assign(path);
  };
  const mobile = event => {
    if (!live() || !event.currentTarget.isConnected || !body.contains(event.currentTarget)
      || event.button !== 0 || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
    event.preventDefault();
    try {
      if (event.currentTarget.dataset.pageMobile === 'disable') window.localStorage.setItem(key, 'true');
      else window.localStorage.removeItem(key);
      window.location.reload();
    } catch {
      if (!notice) {
        notice = document.createElement('p'); notice.className = 'pageChromeStatus'; notice.setAttribute('role', 'status');
        body.prepend(notice);
      }
      notice.textContent = 'The mobile preference could not be saved.';
    }
  };
  const storage = event => { if (event.key === null || event.key === key) refresh(); };
  const hide = event => { if (event.persisted) suspended = true; else destroy(); };
  const show = event => { if (event.persisted && !retired) { suspended = false; refresh(); } };
  function destroy() {
    if (retired) return;
    retired = true;
    select?.removeEventListener('change', change);
    for (const control of controls) control.removeEventListener('click', mobile);
    window.removeEventListener('storage', storage); window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
    if (pageMounts.get(body) === api) pageMounts.delete(body);
    if (body.getAttribute('data-native-never-mobile') === owned) {
      if (previous === null) body.removeAttribute('data-native-never-mobile'); else body.setAttribute('data-native-never-mobile', previous);
    }
    announcement?.destroy();
    notice?.remove();
  }
  if (choices.size && [...choices].every(board => publicBoardPath(board) !== null)) select.addEventListener('change', change);
  for (const control of controls) control.addEventListener('click', mobile);
  window.addEventListener('storage', storage); window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
  api = { refresh, destroy }; pageMounts.set(body, api); refresh();
  return api;
}

if (typeof document !== 'undefined') mountPageChrome(document.body);
