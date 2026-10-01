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
    if (element.getAttribute('title') === (entry.titleHidden ? null : entry.display.title)) {
      if (entry.title === null) element.removeAttribute('title'); else element.setAttribute('title', entry.title);
    }
    entry.release(); dates.delete(element);
  }
  function suppressDateTitle(element) {
    const entry = dates.get(element);
    if (!entry || entry.titleHidden || element.getAttribute('title') !== entry.display.title) return null;
    entry.titleHidden = true; element.removeAttribute('title');
    return () => {
      if (dates.get(element) !== entry || !entry.titleHidden) return;
      entry.titleHidden = false;
      if (element.getAttribute('title') === null) element.title = entry.display.title;
    };
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
        if (!enabled || !root.contains(element) || element.getAttribute(entry.attribute) !== entry.value
          || element.childNodes.length !== entry.children.length
          || entry.children.some((child, index) => element.childNodes[index] !== child)) restoreDate(element, entry);
      }
      if (!enabled) return;
      const walker = document.createTreeWalker(root, window.NodeFilter.SHOW_ELEMENT);
      for (let element = walker.nextNode(), visited = 0; element && visited < DISPLAY_LIMITS.scanNodes; element = walker.nextNode(), visited++) {
        if (dates.size >= DISPLAY_LIMITS.times) break;
        const mobileDate = element.matches('.postInfoM > span.dateTime.postNum[data-utc]');
        if (!element.matches('.postInfo > time[datetime]') && !mobileDate || dates.has(element)
          || element.childNodes.length !== (mobileDate ? 3 : 1) || element.firstChild.nodeType !== 3
          || element.firstChild.data.length > 64 || (element.getAttribute('title')?.length ?? 0) > 128) continue;
        const attribute = mobileDate ? 'data-utc' : 'datetime', value = element.getAttribute(attribute);
        let iso = value;
        if (mobileDate) {
          if (!/^-?(?:0|[1-9][0-9]{0,11})$/.test(value)
            || ![...element.children].every(node => node.localName === 'a')
            || element.children[0].title !== 'Link to this post' || element.children[1].title !== 'Reply to this post') continue;
          const date = new Date(Number(value) * 1000);
          if (!Number.isFinite(date.getTime())) continue;
          iso = date.toISOString();
        }
        const display = localeDate(iso, clock);
        if (!display) continue;
        if (mobileDate) display.text += ' ';
        const text = element.firstChild, original = text.data, title = element.getAttribute('title');
        const release = projection.trackText(text, () => text.data === display.text ? original : text.data);
        dates.set(element, { text, original, title, attribute, value, children: [...element.childNodes], display, release });
        text.data = display.text; element.title = display.title;
      }
    } finally {
      if (!destroyed && !suspended) observer.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['datetime', 'data-utc'] });
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
  return { refresh, openEditor, suppressDateTitle, destroy() {
    if (destroyed) return;
    hide(); destroyed = true;
    window.removeEventListener('storage', storage);
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
    document.removeEventListener('4chanSettingsSaved', refresh);
  } };
}

// Public v1191 IDColor uses the signed 31-based string hash's high three bytes.
export function posterIdColor(id) {
  if (typeof id !== 'string' || !/^[+/0-9A-Za-z]{8}$/.test(id)) return null;
  let hash = 0;
  for (const character of id) hash = (Math.imul(hash, 31) + character.charCodeAt(0)) | 0;
  const red = (hash >>> 24) & 255, green = (hash >>> 16) & 255, blue = (hash >>> 8) & 255;
  return { background: `rgb(${red}, ${green}, ${blue})`,
    color: .299 * red + .587 * green + .114 * blue > 125 ? 'black' : 'white' };
}

export function mountNativePosterIds({ root, settings }) {
  const document = root?.ownerDocument, window = document?.defaultView;
  if (!document || !window || typeof settings !== 'function') return { refresh() {}, destroy() {} };
  const owned = new Map();
  const properties = ['background-color', 'color', 'padding', 'border-radius', 'font-size'];
  let destroyed = false, suspended = false, queued = false;
  function restore(element, entry) {
    for (const [property, previous, priority, applied] of entry) {
      if (element.style.getPropertyValue(property) !== applied) continue;
      if (previous) element.style.setProperty(property, previous, priority);
      else element.style.removeProperty(property);
    }
    owned.delete(element);
  }
  function clear() { for (const [element, entry] of owned) restore(element, entry); }
  function refresh() {
    if (destroyed || suspended) return;
    if (!root.isConnected) { destroy(); return; }
    const config = settings();
    if (config.disableAll === true || config.IDColor === false) { clear(); return; }
    // A finite walk bounds work before collecting a possibly hostile NodeList.
    const walk = document.createTreeWalker(root, window.NodeFilter.SHOW_ELEMENT);
    const found = new Set();
    let element, nodes = 0;
    while ((element = walk.nextNode())) {
      if (++nodes > 40000 || found.size >= 10000) break;
      if (!element.matches('.posteruid > .hand')) continue;
      const color = posterIdColor(element.textContent);
      if (!color) continue;
      found.add(element);
      const previousEntry = owned.get(element);
      if (previousEntry?.label === element.textContent) continue;
      if (previousEntry) restore(element, previousEntry);
      const values = [color.background, color.color, '0px 5px', '6px', '0.8em'];
      const entry = properties.map((property, index) => {
        const previous = element.style.getPropertyValue(property), priority = element.style.getPropertyPriority(property);
        element.style.setProperty(property, values[index]);
        return [property, previous, priority, element.style.getPropertyValue(property)];
      });
      entry.label = element.textContent;
      owned.set(element, entry);
    }
    for (const [element, entry] of owned) if (!found.has(element)) restore(element, entry);
  }
  function schedule() {
    if (queued || destroyed || suspended) return;
    queued = true;
    window.queueMicrotask(() => { queued = false; refresh(); });
  }
  const observer = new window.MutationObserver(schedule);
  function watch() { observer.observe(document.documentElement, { childList: true, subtree: true, characterData: true }); }
  function hide(event) {
    if (!event.persisted) { destroy(); return; }
    suspended = true; observer.disconnect();
  }
  function show(event) { if (event.persisted && !destroyed) { suspended = false; watch(); refresh(); } }
  function destroy() {
    if (destroyed) return;
    destroyed = true; observer.disconnect(); clear();
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
    document.removeEventListener('4chanSettingsSaved', schedule);
    document.removeEventListener('4chanPreferencesRestored', schedule);
  }
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
  document.addEventListener('4chanSettingsSaved', schedule);
  document.addEventListener('4chanPreferencesRestored', schedule);
  watch(); refresh();
  return { refresh, destroy };
}

// Core v1128 toggles ID highlights; extension v1191 counts loaded ID headers.
export function mountNativePosterIdActions({ root, settings, thread = false }) {
  const document = root?.ownerDocument, window = document?.defaultView;
  if (!document || !window || typeof settings !== 'function') return { refresh() {}, destroy() {} };
  const attributes = new Map(), highlighted = new Set();
  let labels = new Map(), selected = null, hovered = null, tooltip = null, timer = null, complete = true;
  let destroyed = false, suspended = false, queued = false;
  function own(element, name, value) {
    let entries = attributes.get(element);
    if (!entries) { entries = new Map(); attributes.set(element, entries); }
    let entry = entries.get(name);
    if (entry && element.getAttribute(name) !== entry.applied) return;
    if (!entry) { entry = { previous: element.getAttribute(name), applied: value }; entries.set(name, entry); }
    entry.applied = value;
    if (element.getAttribute(name) !== value) element.setAttribute(name, value);
  }
  function restore(element, name) {
    const entries = attributes.get(element), entry = entries?.get(name);
    if (!entry) return;
    if (element.getAttribute(name) === entry.applied) {
      if (entry.previous === null) element.removeAttribute(name);
      else element.setAttribute(name, entry.previous);
    }
    entries.delete(name);
    if (!entries.size) attributes.delete(element);
  }
  function restoreAll(element) {
    for (const name of [...(attributes.get(element)?.keys() || [])]) restore(element, name);
  }
  function hideTip() {
    window.clearTimeout(timer); timer = null;
    if (hovered) restore(hovered, 'aria-describedby');
    hovered = null; tooltip?.remove(); tooltip = null;
  }
  function tipAllowed(record) {
    return complete && record?.kind === 'id' && settings().disableAll !== true && (thread || record.section.classList.contains('tExpanded'));
  }
  function showTip() {
    timer = null;
    const record = labels.get(hovered);
    if (!tipAllowed(record) || !hovered.isConnected) { hideTip(); return; }
    const count = new Set([...labels.values()].filter(candidate => candidate.id === record.id).map(candidate => candidate.post)).size;
    if (!tooltip) {
      if (document.getElementById('native-poster-id-tip')) { hideTip(); return; }
      tooltip = document.createElement('span'); tooltip.id = 'native-poster-id-tip';
      tooltip.className = 'poster-id-tooltip'; tooltip.setAttribute('role', 'tooltip'); root.append(tooltip);
      const described = hovered.getAttribute('aria-describedby');
      own(hovered, 'aria-describedby', `${described ? `${described} ` : ''}${tooltip.id}`);
    }
    const text = `${count} post${count === 1 ? '' : 's'} by this ID`;
    if (tooltip.textContent !== text) tooltip.textContent = text;
    const box = hovered.getBoundingClientRect(), tip = tooltip.getBoundingClientRect();
    tooltip.style.left = `${Math.max(0, Math.min(box.left, window.innerWidth - tip.width))}px`;
    tooltip.style.top = `${Math.max(0, Math.min(box.bottom + 4, window.innerHeight - tip.height))}px`;
  }
  function refresh() {
    if (destroyed || suspended) return;
    if (!root.isConnected) { destroy(); return; }
    const found = new Map(), walk = document.createTreeWalker(root, window.NodeFilter.SHOW_ELEMENT);
    let element, nodes = 0;
    complete = true;
    while ((element = walk.nextNode())) {
      if (++nodes > 40000 || found.size >= 10000) { complete = false; break; }
      const badge = element.matches('strong.capcode.hand') && [
        ['Mod', 'capcodeMod', 'id_mod', 'Highlight posts by Moderators'],
        ['Admin', 'capcodeAdmin', 'id_admin', 'Highlight posts by Administrators'],
        ['Founder', 'capcodeAdmin', 'id_admin', 'Highlight posts by the Founder'],
        ['Developer', 'capcodeDeveloper', 'id_developer', 'Highlight posts by Developers'],
        ['Manager', 'capcodeManager', 'id_manager', 'Highlight posts by Managers'],
      ].find(([label, nameClass, group, title]) => element.textContent === `## ${label}`
        && element.className === `capcode hand ${group}` && element.title === title
        && element.parentElement?.className === `nameBlock ${nameClass}`
        && element.parentElement.parentElement?.matches('.postInfo,.postInfoM'));
      const ordinary = element.matches('.posteruid > .hand') && /^[+/0-9A-Za-z]{8}$/.test(element.textContent);
      if (!badge && !ordinary) continue;
      const info = element.closest('.postInfo,.postInfoM'), post = info?.parentElement, section = post?.closest('.thread');
      if (!post?.matches('.post') || !/^p[1-9][0-9]{0,18}$/.test(post.id)
        || !post.parentElement?.matches('.postContainer') || post.parentElement.id !== `pc${post.id.slice(1)}`
        || !/^t[1-9][0-9]{0,18}$/.test(section?.id || '')
        || ![`pi${post.id.slice(1)}`, `pim${post.id.slice(1)}`].includes(info.id)) continue;
      const id = badge ? `capcode:${badge[2]}` : element.textContent;
      found.set(element, { id, kind: badge ? 'capcode' : 'id', post, section });
      own(element, 'role', 'button'); own(element, 'tabindex', '0');
      if (ordinary) own(element, 'title', 'Highlight posts by this ID');
      own(element, 'aria-pressed', String(id === selected));
    }
    for (const element of [...attributes.keys()]) if (!found.has(element)) restoreAll(element);
    labels = found;
    const matching = new Set([...labels.values()].filter(record => record.id === selected).map(record => record.post));
    for (const post of highlighted) if (!matching.has(post)) { post.classList.remove('poster-id-highlight'); highlighted.delete(post); }
    for (const post of matching) if (!post.classList.contains('poster-id-highlight')) {
      post.classList.add('poster-id-highlight'); highlighted.add(post);
    }
    if (hovered && !tipAllowed(labels.get(hovered))) hideTip();
    else if (tooltip) showTip();
  }
  function target(event) { return event.target?.closest?.('.posteruid > .hand,strong.capcode.hand'); }
  function activate(event) {
    const label = target(event) || event.target?.closest?.('.posteruid')?.querySelector(':scope > .hand');
    const record = labels.get(label);
    if (!record || suspended || destroyed) return;
    if (event.type === 'keydown' && (!['Enter', ' '].includes(event.key) || event.repeat)) return;
    if (event.type === 'click' && event.button !== 0) return;
    event.preventDefault(); selected = selected === record.id ? null : record.id; refresh();
  }
  function over(event) {
    const label = target(event);
    if (suspended || destroyed || event.pointerType === 'touch' || !tipAllowed(labels.get(label)) || hovered === label) return;
    hideTip(); hovered = label; timer = window.setTimeout(showTip, 500);
  }
  function out(event) {
    if (target(event) === hovered && !hovered?.contains(event.relatedTarget)) hideTip();
  }
  function schedule() {
    if (queued || destroyed || suspended) return;
    queued = true; window.queueMicrotask(() => { queued = false; refresh(); });
  }
  const observer = new window.MutationObserver(schedule);
  function watch() {
    observer.observe(document.documentElement, { childList: true, subtree: true, characterData: true,
      attributes: true, attributeFilter: ['class'] });
  }
  function hide(event) {
    if (!event.persisted) { destroy(); return; }
    suspended = true; observer.disconnect(); hideTip();
  }
  function show(event) { if (event.persisted && !destroyed) { suspended = false; watch(); refresh(); } }
  function destroy() {
    if (destroyed) return;
    destroyed = true; observer.disconnect(); hideTip();
    for (const element of [...attributes.keys()]) restoreAll(element);
    for (const post of highlighted) post.classList.remove('poster-id-highlight');
    highlighted.clear(); labels.clear();
    root.removeEventListener('click', activate); root.removeEventListener('keydown', activate);
    root.removeEventListener('pointerover', over); root.removeEventListener('pointerout', out);
    root.removeEventListener('focusin', over); root.removeEventListener('focusout', out);
    window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show);
    window.removeEventListener('scroll', hideTip); window.removeEventListener('resize', hideTip);
    document.removeEventListener('4chanSettingsSaved', schedule);
    document.removeEventListener('4chanPreferencesRestored', schedule);
  }
  root.addEventListener('click', activate); root.addEventListener('keydown', activate);
  root.addEventListener('pointerover', over); root.addEventListener('pointerout', out);
  root.addEventListener('focusin', over); root.addEventListener('focusout', out);
  window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
  window.addEventListener('scroll', hideTip, { passive: true }); window.addEventListener('resize', hideTip);
  document.addEventListener('4chanSettingsSaved', schedule); document.addEventListener('4chanPreferencesRestored', schedule);
  watch(); refresh();
  return { refresh, destroy };
}
