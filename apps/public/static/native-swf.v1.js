import { threadSize, indexSize } from './native-swf-core.v1.js';

export function mountFlash({ document: doc, window: win } = {}) {
  const context = doc?.getElementById('watcher-context');
  if (!win || context?.dataset.uploadBoard !== 'true') return null;
  const board = context.dataset.board, thread = context.dataset.thread;
  if (!/^[a-z0-9]{1,10}$/.test(board) || !/^(0|[1-9][0-9]{0,18})$/.test(thread)) return null;
  let origin;
  try {
    origin = new URL(context.dataset.mediaOrigin);
    if (!['http:', 'https:'].includes(origin.protocol) || origin.pathname !== '/' || origin.search || origin.hash || origin.username || origin.password) return null;
    const saved = win.localStorage.getItem('4chan-settings');
    if (saved && saved.length <= 32768 && JSON.parse(saved)?.disableAll === true) return null;
  } catch { if (!origin) return null; }
  const mobile = () => {
    let never = false;
    try { never = win.localStorage.getItem('4chan_never_show_mobile') === 'true'; } catch { /* Public layout default. */ }
    return win.matchMedia('(max-width: 480px)').matches && !never;
  };
  let mounted = null, toggle = null, owner = null;
  const listeners = [];
  const listen = (element, name, handler) => { element.addEventListener(name, handler); listeners.push(() => element.removeEventListener(name, handler)); };
  const close = () => { mounted?.remove(); mounted = null; owner = null; if (toggle) toggle.textContent = 'Embed'; };
  function target(link) {
    try {
      const url = new URL(link.dataset.player);
      const width = Number(link.dataset.width), height = Number(link.dataset.height);
      if (url.origin !== origin.origin || url.search || url.hash || url.username || url.password
        || !new RegExp(`^/player/${board}/[1-9][0-9]{0,15}$`).test(url.pathname)
        || ![width, height].every(value => Number.isSafeInteger(value) && value > 0 && value <= 107374182)) return null;
      return { url: url.href, width, height };
    } catch { return null; }
  }
  function frame(link, size) {
    const value = target(link);
    if (!value || !size || size.width > 8192 || size.height > 8192 || size.width * size.height > 16777216) return null;
    const element = doc.createElement('iframe');
    element.setAttribute('allow', 'autoplay; fullscreen');
    element.setAttribute('sandbox', 'allow-scripts');
    element.setAttribute('scrolling', 'no'); element.setAttribute('frameborder', '0');
    element.width = String(size.width); element.height = String(size.height);
    element.title = 'Flash player'; element.src = value.url;
    return element;
  }
  function threadClick(event, link) {
    event.preventDefault();
    if (mounted) { close(); return; }
    const value = target(link);
    const post = doc.getElementById(`m${thread}`);
    if (!value || !post) return;
    const player = frame(link, threadSize(value.width, value.height, doc.documentElement.clientWidth, mobile()));
    if (!player) return;
    const container = doc.createElement('div'); container.id = 'swf-embed'; container.append(player);
    post.prepend(container); mounted = container; owner = link; toggle.textContent = 'Remove';
    doc.getElementById(`t${thread}`)?.scrollIntoView(true);
  }
  function indexClick(event, link) {
    event.preventDefault(); close();
    const value = target(link);
    if (!value) return;
    const size = indexSize(value.width, value.height, doc.documentElement.clientWidth, doc.documentElement.clientHeight);
    const player = frame(link, size);
    if (!player) return;
    const panel = doc.createElement('div');
    Object.assign(panel.style, { position: 'fixed', width: `${size.width}px`, height: `${size.height}px`, top: '50%', left: '50%', marginTop: `${-size.height / 2 - 10}px`, marginLeft: `${-size.width / 2}px`, background: 'white' });
    const header = doc.createElement('div'); header.id = 'swf-embed-header'; header.className = 'postblock';
    header.textContent = `${link.getAttribute('title') || link.textContent}, ${value.width}x${value.height}`;
    const icon = doc.createElement('img'); icon.id = 'swf-embed-close'; icon.className = 'pointer'; icon.alt = 'Close'; icon.tabIndex = 0;
    let cross = '/static/watcher/futaba/cross.png';
    try {
      const raw = win.getComputedStyle(doc.documentElement).getPropertyValue('--watcher-cross').trim();
      const match = /^url\(["']?([^"')]+)["']?\)$/.exec(raw);
      const url = new URL(match?.[1] || cross, win.location.href);
      if (url.origin === win.location.origin && /^\/static\/watcher\/[a-z-]+\/cross(?:@2x)?\.png$/.test(url.pathname) && !url.search && !url.hash) cross = url.href;
    } catch { /* Use the release-owned default icon. */ }
    icon.src = cross; header.append(icon); panel.append(header, player);
    const backdrop = doc.createElement('div'); backdrop.id = 'swf-embed';
    Object.assign(backdrop.style, { width: '100%', height: '100%', position: 'fixed', top: '0', left: '0', background: 'rgba(128, 128, 128, 0.5)' });
    backdrop.append(panel); backdrop.addEventListener('click', event => { if (event.target === backdrop || event.target === icon) close(); });
    icon.addEventListener('keydown', event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); close(); } });
    doc.body.append(backdrop); mounted = backdrop; owner = link;
  }
  if (thread !== '0') {
    const text = doc.getElementById(`fT${thread}`), link = text?.querySelector('a[data-player]');
    if (link && target(link) && !text.querySelector('[data-swf-toggle]')) {
      toggle = doc.createElement('a'); toggle.href = '#'; toggle.textContent = 'Embed'; toggle.dataset.swfToggle = 'true';
      text.append('-[', toggle, ']'); listen(toggle, 'click', event => threadClick(event, link));
    }
  } else if (!mobile()) {
    const table = doc.querySelector('table.flashListing');
    const rows = [...(table?.rows || [])].slice(0, 501);
    if (rows.length && rows[0].children.length >= 3 && !table.dataset.swfMounted) {
      table.dataset.swfMounted = 'true';
      const column = doc.createElement('td'); column.className = 'postblock'; rows[0].children[2].after(column);
      for (const row of rows.slice(1)) {
        const link = row.children[2]?.querySelector('a[data-player]');
        if (!link || !target(link)) continue;
        const cell = doc.createElement('td'), control = doc.createElement('a'); control.href = link.href; control.textContent = 'Embed';
        cell.append('[', control, ']'); row.children[2].after(cell); listen(control, 'click', event => indexClick(event, link));
      }
    }
  }
  const observer = new win.MutationObserver(() => { if (owner && !owner.isConnected) close(); });
  const root = doc.querySelector('.board'); if (root) observer.observe(root, { childList: true, subtree: true });
  listen(win, 'pagehide', close);
  return { close, destroy() { close(); observer.disconnect(); for (const remove of listeners) remove(); } };
}

if (typeof document !== 'undefined' && typeof window !== 'undefined') mountFlash({ document, window });
