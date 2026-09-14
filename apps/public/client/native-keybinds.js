const bindings = new Map([[65, 'auto'], [70, 'filter'], [81, 'quickReply'], [82, 'update'],
  [87, 'watch'], [66, 'previous'], [67, 'catalog'], [78, 'next'], [73, 'index']]);

export function nativeShortcut(event, settings) {
  if (settings?.disableAll === true || settings?.keyBinds !== true
    || ['TEXTAREA', 'INPUT'].includes(event.target?.nodeName)
    || event.altKey || event.shiftKey || event.ctrlKey || event.metaKey) return null;
  return bindings.get(event.keyCode) ?? null;
}

export function siblingPageUrl(origin, board, href) {
  if (typeof board !== 'string' || !/^[a-z0-9]{1,10}$/.test(board) || typeof href !== 'string') return null;
  try {
    const base = new URL(origin), url = new URL(href, base);
    const path = new RegExp(`^/${board}/(?:(0|[1-9][0-9]{0,9}))?/?$`).exec(url.pathname);
    if (!['http:', 'https:'].includes(base.protocol) || base.username || base.password
      || url.origin !== base.origin || url.username || url.password || url.search || url.hash
      || !path || (path[1] && Number(path[1]) > 2147483647)) return null;
    return url.href;
  } catch { return null; }
}

export function mountNativeKeybinds({ board, settings, watch, filter, update, auto }) {
  let help = null, opener = null;
  const page = direction => {
    const link = document.querySelector(`.pages > a[rel="${direction}"]`);
    const url = link && siblingPageUrl(location.origin, board, link.getAttribute('href'));
    if (url) location.assign(url);
  };
  const actions = { watch, filter, update, auto, previous: () => page('prev'), next: () => page('next'),
    index: () => location.assign(`/${board}/`), catalog: () => location.assign(`/${board}/catalog`) };
  const resolve = event => {
    const action = nativeShortcut(event, settings());
    if (!action) return;
    event.preventDefault(); event.stopPropagation();
    actions[action]?.();
  };
  function close() {
    if (!help) return;
    help.close(); help.remove(); help = null; opener?.focus();
  }
  function openHelp(source) {
    if (help) return;
    opener = source;
    const dialog = document.createElement('dialog');
    dialog.id = 'keybindsHelp'; dialog.className = 'nativeSettings extensionSettings UIPanel nativeKeybindsHelp';
    dialog.setAttribute('aria-labelledby', 'keybinds-title');
    const panel = document.createElement('div'); panel.className = 'extPanel nativeSettingsBody';
    const header = document.createElement('h2'); header.className = 'panelHeader';
    const title = document.createElement('span'); title.id = 'keybinds-title'; title.textContent = 'Keyboard Shortcuts';
    const dismiss = document.createElement('button'); dismiss.type = 'button'; dismiss.className = 'panelCtrl';
    dismiss.id = 'keybinds-close'; dismiss.textContent = '\u00d7'; dismiss.setAttribute('aria-label', 'Close keyboard shortcuts');
    dismiss.addEventListener('click', close); header.append(title, dismiss); panel.append(header);
    const list = document.createElement('ul');
    for (const [key, label] of [['W', 'Watch/Unwatch thread'], ['B', 'Previous page'], ['N', 'Next page'],
      ['I', 'Return to index'], ['C', 'Open catalog'], ['F', 'Filter selected text'], ['R', 'Update thread'], ['A', 'Toggle auto-updater']]) {
      const row = document.createElement('li'), keycap = document.createElement('kbd');
      keycap.textContent = key; row.append(keycap, ` - ${label}`); list.append(row);
    }
    const pending = document.createElement('p'); pending.className = 'settings-tip';
    pending.textContent = 'Quick Reply (Q) is not available yet.';
    panel.append(list, pending); dialog.append(panel);
    dialog.addEventListener('cancel', event => { event.preventDefault(); close(); });
    document.body.append(dialog); help = dialog; dialog.showModal(); dismiss.focus();
  }
  document.addEventListener('keydown', resolve);
  window.addEventListener('pagehide', () => document.removeEventListener('keydown', resolve), { once: true });
  return { openHelp };
}
