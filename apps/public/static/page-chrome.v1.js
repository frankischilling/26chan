// Fixed public navigation; board data and destination paths stay local.
const pageMounts = new WeakMap();
export function publicBoardPath(board, catalog = false) {
  if (typeof board !== 'string' || !/^[a-z0-9]{1,10}$/.test(board) || typeof catalog !== 'boolean') return null;
  return `/${board}/${catalog && board !== 'f' ? 'catalog' : ''}`;
}

export function mountPageChrome(body) {
  const document = body?.ownerDocument, window = document?.defaultView;
  if (!window || !(body instanceof window.HTMLBodyElement) || document.body !== body || !body.isConnected
    || !body.classList.contains('publicPageChrome') || !['true', 'false'].includes(body.dataset.pageCatalog)) return null;
  pageMounts.get(body)?.destroy();
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
    notice?.remove();
  }
  if (choices.size && [...choices].every(board => publicBoardPath(board) !== null)) select.addEventListener('change', change);
  for (const control of controls) control.addEventListener('click', mobile);
  window.addEventListener('storage', storage); window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
  api = { refresh, destroy }; pageMounts.set(body, api); refresh();
  return api;
}

if (typeof document !== 'undefined') mountPageChrome(document.body);
