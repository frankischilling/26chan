// Release-owned display controls. Preferences supply text and local board names.
export const DISPLAY_LIMITS = Object.freeze({ listBytes: 1024, boards: 64, menus: 2, times: 10000, scanNodes: 40000 });

export function customBoards(value) {
  if (typeof value !== 'string' || value.length > DISPLAY_LIMITS.listBytes
    || new TextEncoder().encode(value).length > DISPLAY_LIMITS.listBytes) return null;
  const boards = value.split(/[^0-9a-z]+/i).filter(Boolean);
  if (boards.length > DISPLAY_LIMITS.boards || boards.some(board => board.length > 10)) return null;
  return boards.map(board => board.toLowerCase());
}

export function localeDate(value, now = new Date()) {
  if (typeof value !== 'string' || value.length > 64
    || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/.test(value)) return null;
  const date = new Date(value);
  if (!Number.isFinite(date.getTime()) || !(now instanceof Date) || !Number.isFinite(now.getTime())) return null;
  const two = number => `0${number}`.slice(-2);
  const offset = now.getTimezoneOffset(), minutes = Math.abs(offset);
  return {
    text: `${two(date.getMonth() + 1)}/${two(date.getDate())}/${two(date.getFullYear())}`
      + `(${['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][date.getDay()]})`
      + `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`,
    title: `Timezone: UTC${offset ? `${offset < 0 ? '+' : '-'}${Math.floor(minutes / 60)}${minutes % 60 ? `:${minutes % 60}` : ''}` : ''}`,
  };
}

export function mountNativeDisplay({ root, settings, save, openSettings, projection }) {
  const document = root?.ownerDocument, window = document?.defaultView;
  if (!root || !window || !projection?.trackText) return { refresh() {}, openEditor() {}, destroy() {} };
  const dates = new Map(), menus = [];
  let active = null, suspended = false, destroyed = false, queued = false;
  let menuKey = null, showAllKey = null;
  const clock = new Date();
  const node = (tag, text, className) => {
    const element = document.createElement(tag);
    if (text !== undefined) element.textContent = text;
    if (className) element.className = className;
    return element;
  };
  function link(text, title, action) {
    const element = node('a', text);
    element.href = '#'; element.title = title;
    element.addEventListener('click', event => { event.preventDefault(); action(element); });
    return element;
  }
  function restoreDate(element, entry) {
    if (entry.text.parentNode === element && entry.text.data === entry.display.text) entry.text.data = entry.original;
    if (element.getAttribute('title') === entry.display.title) {
      if (entry.title === null) element.removeAttribute('title'); else element.setAttribute('title', entry.title);
    }
    entry.release(); dates.delete(element);
  }
  function restoreMenus() {
    for (const { original, hidden, menu } of menus.splice(0)) {
      original.hidden = hidden; menu.remove();
    }
  }
  function closeEditor(focus = true) {
    const current = active;
    if (!current) return;
    active = null; current.dialog.close(); current.dialog.remove();
    if (focus && current.opener?.isConnected) current.opener.focus();
  }
  function openEditor(opener) {
    if (destroyed || suspended || settings().disableAll === true) return;
    closeEditor(false);
    const dialog = node('dialog', undefined, 'nativeSettings extensionSettings UIPanel');
    dialog.id = 'customMenu'; dialog.setAttribute('aria-labelledby', 'custom-menu-title');
    const body = node('div', undefined, 'extPanel nativeSettingsBody');
    const heading = node('h2', 'Custom Board List', 'panelHeader'); heading.id = 'custom-menu-title';
    const form = node('form'), caption = node('label', 'Boards'); caption.htmlFor = 'customMenuBox';
    const input = node('input'); input.id = 'customMenuBox'; input.type = 'text';
    input.maxLength = DISPLAY_LIMITS.listBytes; input.placeholder = 'Example: test demo';
    const previous = settings().customMenuList;
    if (typeof previous === 'string' && customBoards(previous) !== null) input.value = previous;
    const message = node('p', '', 'settingsMessage'); message.setAttribute('role', 'status');
    const submit = node('button', 'Save board list'); submit.type = 'submit';
    const cancel = node('button', 'Cancel'); cancel.type = 'button'; cancel.addEventListener('click', () => closeEditor());
    form.append(caption, ' ', input, message, submit, ' ', cancel);
    form.addEventListener('submit', async event => {
      event.preventDefault();
      if (submit.disabled || active?.dialog !== dialog) return;
      if (customBoards(input.value) === null) {
        message.textContent = 'Use at most 64 board names, each at most 10 letters or digits, within 1,024 bytes.'; return;
      }
      submit.disabled = true;
      try {
        const result = await save({ customMenu: true, customMenuList: input.value });
        if (active?.dialog !== dialog) return;
        if (result === false) { message.textContent = 'The board list could not be saved. Try again.'; return; }
        showAllKey = null; closeEditor(); refresh();
        document.dispatchEvent(new window.CustomEvent('4chanSettingsSaved'));
      } catch { if (active?.dialog === dialog) message.textContent = 'The board list could not be saved. Try again.'; }
      finally { submit.disabled = false; }
    });
    dialog.addEventListener('cancel', event => { event.preventDefault(); closeEditor(); });
    body.append(heading, form); dialog.append(body); document.body.append(dialog);
    active = { dialog, opener }; dialog.showModal(); input.focus();
  }
  function navigation(config) {
    const key = JSON.stringify([config.customMenu === true, config.customMenuList]);
    const boards = config.customMenu === true && config.disableAll !== true ? customBoards(config.customMenuList) : null;
    if (!boards?.length || showAllKey === key) { restoreMenus(); menuKey = key; return; }
    if (menuKey === key && menus.length && menus.every(entry => entry.menu.isConnected)) return;
    restoreMenus(); menuKey = key;
    const originals = document.querySelectorAll('nav.boardList:not(.customBoardList)');
    for (const original of Array.from(originals).slice(0, DISPLAY_LIMITS.menus)) {
      const menu = node('nav', undefined, 'boardList customBoardList');
      menu.setAttribute('aria-label', 'Custom board navigation'); menu.append('[ ');
      boards.forEach((board, index) => {
        if (index) menu.append(' / ');
        const anchor = node('a', board); anchor.href = `/${board}/`; menu.append(anchor);
      });
      const all = link('\u2026', 'Show all', () => { showAllKey = key; refresh(); });
      all.className = 'show-all-boards'; all.setAttribute('aria-label', 'Show all boards');
      const edit = link('Edit', 'Edit Menu', openEditor); edit.setAttribute('aria-haspopup', 'dialog');
      const settingsLink = link('Settings', 'Settings', source => openSettings?.(source));
      settingsLink.setAttribute('aria-haspopup', 'dialog');
      menu.append(' ] [ ', all, ' ] [ ', edit, ' ] [ ', settingsLink, ' ]');
      menus.push({ original, hidden: original.hidden, menu });
      original.hidden = true; original.before(menu);
    }
  }
  function refresh() {
    if (destroyed || suspended) return;
    observer.disconnect();
    try {
      const config = settings(), enabled = config.localTime !== false && config.disableAll !== true;
      if (config.disableAll === true) closeEditor();
      navigation(config);
      for (const [element, entry] of dates) {
        if (!enabled || !root.contains(element) || element.getAttribute('datetime') !== entry.iso
          || element.childNodes.length !== 1 || element.firstChild !== entry.text) restoreDate(element, entry);
      }
      if (!enabled) return;
      const walker = document.createTreeWalker(root, window.NodeFilter.SHOW_ELEMENT);
      for (let element = walker.nextNode(), visited = 0; element && visited < DISPLAY_LIMITS.scanNodes; element = walker.nextNode(), visited++) {
        if (dates.size >= DISPLAY_LIMITS.times) break;
        if (!element.matches('.postInfo > time[datetime]') || dates.has(element)
          || element.childNodes.length !== 1 || element.firstChild.nodeType !== 3
          || element.firstChild.data.length > 64 || (element.getAttribute('title')?.length ?? 0) > 128) continue;
        const iso = element.getAttribute('datetime'), display = localeDate(iso, clock);
        if (!display) continue;
        const text = element.firstChild, original = text.data, title = element.getAttribute('title');
        const release = projection.trackText(text, () => text.data === display.text ? original : text.data);
        dates.set(element, { text, original, title, iso, display, release });
        text.data = display.text; element.title = display.title;
      }
    } finally {
      if (!destroyed && !suspended) observer.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['datetime'] });
    }
  }
  function schedule() {
    if (queued || destroyed || suspended) return;
    queued = true;
    window.queueMicrotask(() => { queued = false; refresh(); });
  }
  const observer = new window.MutationObserver(schedule);
  const storage = event => { if (event.key === null || event.key === '4chan-settings') refresh(); };
  const hide = () => {
    suspended = true; observer.disconnect(); closeEditor(false); restoreMenus();
    for (const [element, entry] of dates) restoreDate(element, entry);
  };
  const show = () => { suspended = false; refresh(); };
  window.addEventListener('storage', storage);
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
  document.addEventListener('4chanSettingsSaved', refresh);
  refresh();
  return { refresh, openEditor, destroy() {
    if (destroyed) return;
    hide(); destroyed = true;
    window.removeEventListener('storage', storage);
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
    document.removeEventListener('4chanSettingsSaved', refresh);
  } };
}
