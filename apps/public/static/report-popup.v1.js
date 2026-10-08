// Deliberately dependency-free: report pages permit only this script in CSP.
const validBoard = value => typeof value === 'string' && /^[a-z0-9]{1,10}$/.test(value);
const postId = value => typeof value === 'string' && /^[1-9][0-9]{0,18}$/.test(value)
  && BigInt(value) <= 9223372036854775807n ? value : null;
const CLOSE_MS = 3000;

export function mountReportPopup({ window: win, document: doc }) {
  const root = doc.getElementById('report-popup-context');
  if (!root) return null;
  const { board, post: id } = root.dataset;
  const origin = win.location.origin;
  const validTarget = validBoard(board) && !!postId(id);
  if (!validBoard(board)) return null;
  function opener() {
    try {
      const parent = win.opener;
      return parent && parent !== win && !parent.closed && parent.location.origin === origin ? parent : null;
    } catch { return null; }
  }
  // A regular report tab has no popup name. Even an opener-bearing ordinary tab
  // keeps its navigation and is never closed by the success timer.
  const namedTarget = /^report-popup-([a-z0-9]{1,10})-([1-9][0-9]{0,18})-(.+)$/.exec(win.name);
  const popup = !!namedTarget && namedTarget[1] === board && !!postId(namedTarget[2]) && !!opener();
  const matchingTarget = popup && validTarget && namedTarget[2] === id;
  function returnURL() {
    try {
      const value = doc.getElementById('report-popup-return')?.getAttribute('href');
      if (!value) return null;
      const url = new URL(value, origin);
      if (url.origin !== origin || url.username || url.password || url.search) return null;
      if (url.pathname === `/${board}/` && !url.hash) return url.href;
      const match = new RegExp(`^/${board}/thread/([1-9][0-9]{0,18})$`).exec(url.pathname);
      return match && postId(match[1]) && url.hash === `#p${id}` ? url.href : null;
    } catch { return null; }
  }
  function closeWindow() {
    try { win.close(); } catch { /* Keep the native Return link usable if closing is denied. */ }
  }
  function close() {
    if (popup && opener()) closeWindow();
    else {
      const url = returnURL();
      if (url) win.location.assign(url);
    }
  }
  function keydown(event) {
    if (popup && opener() && event.key === 'Escape' && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey) {
      event.preventDefault(); close();
    }
  }
  const rule = doc.getElementById('report-category-rule');
  const illegal = doc.getElementById('report-category-illegal');
  const category = doc.getElementById('report-category-select');
  function syncCategory() {
    if (category) category.disabled = !!illegal?.checked;
  }
  rule?.addEventListener('change', syncCategory);
  illegal?.addEventListener('change', syncCategory);
  syncCategory();
  const control = doc.getElementById('report-popup-close');
  if (control) control.hidden = false;
  control?.addEventListener('click', close);
  doc.addEventListener('keydown', keydown);
  let timer = null;
  if (matchingTarget && root.dataset.result === 'success') {
    try {
      opener()?.postMessage(`done-report-${id}-${board}`, origin);
      timer = win.setTimeout(() => { if (opener()) closeWindow(); }, CLOSE_MS);
    } catch { /* A closed or navigated opener cannot receive a success signal. */ }
  }
  function destroy() {
    if (timer !== null) win.clearTimeout(timer);
    control?.removeEventListener('click', close);
    rule?.removeEventListener('change', syncCategory);
    illegal?.removeEventListener('change', syncCategory);
    doc.removeEventListener('keydown', keydown);
    win.removeEventListener('pagehide', destroy);
  }
  win.addEventListener('pagehide', destroy);
  return { destroy };
}

if (typeof document !== 'undefined' && typeof window !== 'undefined') mountReportPopup({ window, document });
