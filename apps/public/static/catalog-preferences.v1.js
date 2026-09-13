(() => {
  'use strict';
  const form = document.getElementById('ctrl');
  const order = document.getElementById('order-ctrl');
  const size = document.getElementById('size-ctrl');
  const teaser = document.getElementById('teaser-ctrl');
  const reset = document.getElementById('catalog-reset');
  const search = document.getElementById('qf-box');
  if (!(form instanceof HTMLFormElement) || !(order instanceof HTMLSelectElement)
      || !(size instanceof HTMLSelectElement) || !(teaser instanceof HTMLSelectElement)
      || !(reset instanceof HTMLAnchorElement)) return;

  const key = 'catalog-settings';
  const searchKey = '4chan-catalog-search';
  const boardKey = '4chan-catalog-search-board';
  const orders = ['alt', 'absdate', 'date', 'r'];
  const current = () => ({ orderby: order.value, large: size.value === 'large', extended: teaser.value === 'on' });
  const valid = value => value !== null && typeof value === 'object' && !Array.isArray(value)
    && orders.includes(value.orderby) && typeof value.large === 'boolean' && typeof value.extended === 'boolean';
  const validQuery = value => typeof value === 'string' && value.length <= 256
    && Array.from(value).length <= 128 && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
  const container = document.getElementById('threads');
  const hidden = document.getElementById('catalogFiltered');
  const action = new URL(form.action);
  const board = action.pathname.split('/')[1];
  const integer = (value, signed = false) => {
    if (typeof value !== 'string' || !(signed ? /^-?[0-9]{1,20}$/ : /^[0-9]{1,20}$/).test(value)) throw new Error('Invalid catalog rank');
    return BigInt(value);
  };
  const dimensions = (thumb, mode, maximum) => ['Width', 'Height'].map(axis => {
    const value = thumb.dataset[mode + axis];
    if (typeof value !== 'string' || !/^[0-9]{1,3}$/.test(value)) throw new Error('Invalid thumbnail dimension');
    const number = Number(value);
    if (number < 1 || number > maximum) throw new Error('Invalid thumbnail dimension');
    return number;
  });
  let entries = null;
  let hiddenCount = 0;
  try {
    if (!(container instanceof HTMLElement)) throw new Error('Missing catalog');
    const nodes = Array.from(container.querySelectorAll(':scope > .thread'));
    if (hidden instanceof HTMLTemplateElement) {
      const excluded = Array.from(hidden.content.children).filter(node => node.matches('.thread'));
      hiddenCount = excluded.length;
      nodes.push(...excluded);
    }
    entries = nodes.map(node => {
      const data = node.dataset;
      if (!['true', 'false'].includes(data.sticky)) throw new Error('Invalid sticky flag');
      const teaserNode = node.querySelector(':scope > .teaser')
        ?? node.querySelector(':scope > template.catalogTeaser')?.content.querySelector('.teaser');
      if (!(teaserNode instanceof HTMLElement)) throw new Error('Missing teaser');
      const thumb = node.querySelector('.catalogThumb img[id^="thumb-"]');
      const fields = node.querySelector('.catalogThumb')?.dataset;
      const searchable = fields && ['true', 'false'].includes(fields.hasFile)
        && ['searchSubject', 'searchComment', 'searchFile'].every(name => typeof fields[name] === 'string');
      return { node, teaser: teaserNode, thumb,
        fields: searchable ? [fields.searchSubject, fields.searchComment, ...(fields.hasFile === 'true' ? [fields.searchFile] : [])] : null,
        small: thumb ? dimensions(thumb, 'small', 150) : null,
        large: thumb ? dimensions(thumb, 'large', 250) : null,
        id: integer(data.threadId), bumped: integer(data.bumped, true),
        latest: data.latestReply === '' ? null : integer(data.latestReply), replies: integer(data.replies), sticky: data.sticky === 'true' };
    });
  } catch { entries = null; }
  const searchReady = entries !== null && search instanceof HTMLInputElement
    && hidden instanceof HTMLTemplateElement && entries.every(entry => entry.fields !== null)
    && action.origin === location.origin && action.pathname === location.pathname;
  const originalEmpty = container?.querySelector(':scope > .empty');
  let renderedOrder = hiddenCount ? null : order.value;
  let renderedQuery = search instanceof HTMLInputElement ? search.value : '';
  const compare = (a, b) => a < b ? -1 : a > b ? 1 : 0;
  const compareOptional = (a, b) => a === null ? (b === null ? 0 : -1) : b === null ? 1 : compare(a, b);
  const escape = new RegExp('(' + ['/', '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '\\'].map(character => '\\' + character).join('|') + ')', 'g');
  let cachedQuery = null;
  let pattern = null;
  let timer;
  let composing = false;

  const stateReady = searchReady && entries.every(entry => entry.replies <= BigInt(Number.MAX_SAFE_INTEGER)
    && entry.node.querySelector('.meta > b, .meta > i > b') && entry.node.querySelector('a.catalogThumb[href]'));
  const pinKey = `4chan-pin-${board}`;
  const hideKey = `4chan-hide-t-${board}`;
  const byId = new Map((entries ?? []).map(entry => [entry.id.toString(), entry]));
  const newest = (entries ?? []).reduce((maximum, entry) => entry.id > maximum ? entry.id : maximum, 0n);
  const persistState = (name, values) => {
    try {
      if (values.size) localStorage.setItem(name, JSON.stringify(Object.fromEntries(values)));
      else localStorage.removeItem(name);
    } catch { /* Thread state remains usable in memory without storage. */ }
  };
  const readState = (name, pins) => {
    const values = new Map();
    if (!stateReady) return values;
    try {
      const raw = localStorage.getItem(name);
      if (raw === null) return values;
      if (raw.length > 65536) throw new Error('Oversized thread state');
      const parsed = JSON.parse(raw);
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('Invalid thread state');
      const pairs = Object.entries(parsed);
      if (pairs.length > 1024) throw new Error('Too many stored threads');
      for (const [id, value] of pairs) {
        if (!/^[1-9][0-9]{0,18}$/.test(id) || BigInt(id) > 9223372036854775807n) continue;
        if (pins ? !Number.isSafeInteger(value) || value < 0 : value !== true) continue;
        if (!byId.has(id) && BigInt(id) < newest) continue;
        values.set(id, value);
      }
    } catch { /* Invalid optional state cannot disable the catalog. */ }
    persistState(name, values);
    return values;
  };
  const pins = readState(pinKey, true);
  const hiddenThreads = readState(hideKey, false);
  const remember = (values, id, value) => {
    if (!values.has(id) && values.size >= 1024) {
      const absent = Array.from(values.keys()).find(key => !byId.has(key));
      values.delete(absent ?? values.keys().next().value);
    }
    values.set(id, value);
  };
  const pageSize = Number(container?.dataset.threadsPerPage);
  const pages = new Map();
  if (stateReady && Number.isInteger(pageSize) && pageSize >= 1 && pageSize <= 1000) {
    [...entries].sort((a, b) => Number(b.sticky) - Number(a.sticky)
      || compare(b.bumped, a.bumped) || compare(b.id, a.id))
      .forEach((entry, index) => pages.set(entry.id.toString(), 1 + Math.floor(index / pageSize)));
  }
  let hiddenOnly = false;
  let shownHiddenCount = 0;
  const hiddenLabels = [];
  let unpinAll;
  let menu;
  let menuButton;
  const closeMenu = (restoreFocus = false) => {
    const button = menuButton;
    menu?.remove();
    button?.classList.remove('menuOpen');
    button?.closest('.thread')?.classList.remove('catalogMenuActive');
    button?.setAttribute('aria-expanded', 'false');
    menu = null;
    menuButton = null;
    if (restoreFocus && button?.isConnected) button.focus();
  };
  const updateStateControls = () => {
    for (const { label, count, toggle } of hiddenLabels) {
      label.hidden = shownHiddenCount === 0;
      count.textContent = String(shownHiddenCount);
      toggle.textContent = hiddenOnly ? 'Back' : 'Show';
      toggle.setAttribute('aria-pressed', String(hiddenOnly));
    }
    if (unpinAll) unpinAll.hidden = pins.size === 0;
  };
  const togglePin = entry => {
    closeMenu(true);
    const id = entry.id.toString();
    if (pins.has(id)) pins.delete(id);
    else remember(pins, id, Number(entry.replies));
    persistState(pinKey, pins);
    renderedOrder = null;
    apply(current());
  };
  const toggleHidden = (entry, unhide = hiddenOnly) => {
    closeMenu();
    const id = entry.id.toString();
    if (unhide) hiddenThreads.delete(id);
    else remember(hiddenThreads, id, true);
    persistState(hideKey, hiddenThreads);
    if (unhide && !hiddenOnly) { apply(current()); return; }
    entry.node.remove();
    shownHiddenCount += unhide ? -1 : 1;
    if (hiddenOnly && shownHiddenCount === 0) { hiddenOnly = false; apply(current()); }
    else if (!hiddenOnly && !renderedQuery && !container.querySelector(':scope > .thread')) apply(current());
    else updateStateControls();
  };
  const openMenu = (entry, button) => {
    if (menuButton === button) { closeMenu(true); return; }
    closeMenu();
    menuButton = button;
    menu = document.createElement('div');
    menu.id = 'post-menu';
    menu.className = 'dd-menu';
    menu.setAttribute('role', 'menu');
    menu.setAttribute('aria-label', 'Thread actions');
    const list = document.createElement('ul');
    list.setAttribute('role', 'none');
    const addItem = (label, action, href) => {
      const item = document.createElement('li');
      item.setAttribute('role', 'none');
      const control = document.createElement(href ? 'a' : 'button');
      if (href) control.href = href;
      else { control.type = 'button'; control.addEventListener('click', action); }
      control.setAttribute('role', 'menuitem');
      control.textContent = label;
      item.append(control);
      list.append(item);
    };
    const id = entry.id.toString();
    const report = new URL(entry.node.querySelector('a.catalogThumb').href);
    report.hash = `report${id}`;
    addItem('Report thread', null, report.href);
    addItem(pins.has(id) ? 'Unpin thread' : 'Pin thread', () => togglePin(entry));
    addItem(hiddenThreads.has(id) ? 'Unhide thread' : 'Hide thread', () => {
      toggleHidden(entry, hiddenThreads.has(id));
      if (hiddenLabels[0] && !hiddenLabels[0].label.hidden) hiddenLabels[0].toggle.focus();
    });
    menu.append(list);
    entry.node.querySelector('.meta').append(menu);
    entry.node.classList.add('catalogMenuActive');
    button.classList.add('menuOpen');
    button.setAttribute('aria-expanded', 'true');
    menu.addEventListener('keydown', event => {
      const items = Array.from(menu.querySelectorAll('[role="menuitem"]'));
      const index = items.indexOf(document.activeElement);
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeMenu(true); }
      else if (event.key === 'Tab') closeMenu(true);
      else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault();
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1
          : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
        items[next].focus();
      }
    });
    list.querySelector('[role="menuitem"]').focus();
  };
  const installThreadControls = () => {
    for (const entry of entries) {
      const meta = entry.node.querySelector('.meta');
      const reply = meta.querySelector('b');
      entry.pinDelta = document.createElement('span');
      entry.pinDelta.className = 'catalogPinDelta';
      entry.pinDelta.hidden = true;
      (reply.closest('i') ?? reply).after(entry.pinDelta);
      entry.pinPage = document.createElement('span');
      entry.pinPage.className = 'catalogPinPage';
      entry.pinPage.hidden = true;
      const number = document.createElement('b');
      number.textContent = String(pages.get(entry.id.toString()) ?? '');
      entry.pinPage.append(' / P: ', number);
      meta.append(entry.pinPage);
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'postMenuBtn';
      button.textContent = '\u25b6';
      button.title = 'Thread Menu';
      button.setAttribute('aria-label', `Thread ${entry.id} menu`);
      button.setAttribute('aria-haspopup', 'menu');
      button.setAttribute('aria-expanded', 'false');
      button.addEventListener('click', event => { event.stopPropagation(); openMenu(entry, button); });
      meta.append(button);
    }
    for (const suffix of ['', '-bottom']) {
      const label = document.createElement('span');
      label.id = `hidden-label${suffix}`;
      label.className = 'catalogState';
      label.hidden = true;
      const count = document.createElement('span');
      count.id = `hidden-count${suffix}`;
      const toggle = document.createElement('button');
      toggle.type = 'button';
      toggle.id = `filters-clear-hidden${suffix}`;
      toggle.addEventListener('click', () => { hiddenOnly = !hiddenOnly; closeMenu(); apply(current()); });
      label.append('Hidden: ', count, ' [', toggle, ']');
      hiddenLabels.push({ label, count, toggle });
      if (suffix) container.after(label);
      else form.append(label);
    }
    unpinAll = document.createElement('button');
    unpinAll.type = 'button';
    unpinAll.id = 'catalog-unpin-all';
    unpinAll.textContent = 'Unpin all threads';
    unpinAll.hidden = true;
    unpinAll.addEventListener('click', () => {
      closeMenu(); pins.clear(); persistState(pinKey, pins); renderedOrder = null; apply(current());
    });
    form.append(unpinAll);
    container.addEventListener('click', event => {
      const link = event.target instanceof Element ? event.target.closest('a.catalogThumb') : null;
      if (event.button !== 0 || !(event.altKey || event.shiftKey) || !link || !container.contains(link)) return;
      const entry = byId.get(link.closest('.thread')?.dataset.threadId);
      if (!entry) return;
      event.preventDefault();
      if (event.altKey) togglePin(entry);
      else toggleHidden(entry);
    });
    container.addEventListener('contextmenu', event => {
      const link = event.target instanceof Element ? event.target.closest('a.catalogThumb') : null;
      const entry = link && byId.get(link.closest('.thread')?.dataset.threadId);
      if (!entry) return;
      event.preventDefault();
      openMenu(entry, entry.node.querySelector('.postMenuBtn'));
    });
    document.addEventListener('click', event => {
      if (menu && !menu.contains(event.target) && event.target !== menuButton) closeMenu();
    });
    updateStateControls();
  };

  const save = () => {
    const value = current();
    if (!valid(value)) return;
    try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* Storage is optional. */ }
  };
  const saveSearch = query => {
    try {
      if (query) {
        sessionStorage.setItem(searchKey, query);
        sessionStorage.setItem(boardKey, board);
      } else {
        sessionStorage.removeItem(searchKey);
        sessionStorage.removeItem(boardKey);
      }
    } catch { /* Search does not depend on storage. */ }
  };
  const setOptions = (url, value) => {
    url.searchParams.set('order', value.orderby);
    url.searchParams.set('size', value.large ? 'large' : 'small');
    url.searchParams.set('teaser', value.extended ? 'on' : 'off');
  };
  const updateURL = value => {
    const url = new URL(location.href);
    setOptions(url, value);
    if (searchReady) {
      if (renderedQuery) url.searchParams.set('q', renderedQuery);
      else url.searchParams.delete('q');
      if (url.hash.startsWith('#s=')) url.hash = renderedQuery ? `s=${encodeURIComponent(renderedQuery)}` : '';
    }
    try { history.replaceState(null, '', url.href); } catch { /* Display still works without history. */ }
  };
  const apply = (value, query = renderedQuery) => {
    if (entries === null || !valid(value) || (searchReady && !validQuery(query))) return false;
    closeMenu();
    if (stateReady) {
      const total = entries.filter(entry => hiddenThreads.has(entry.id.toString())).length;
      if (hiddenOnly && total === 0) hiddenOnly = false;
      shownHiddenCount = hiddenOnly || !query ? total : 0;
    }
    if (value.orderby !== renderedOrder) {
      entries.sort((a, b) => Number(b.sticky) - Number(a.sticky)
        || (stateReady ? Number(pins.has(b.id.toString())) - Number(pins.has(a.id.toString())) : 0) || (
        value.orderby === 'date' ? compare(b.id, a.id)
          : value.orderby === 'absdate' ? compareOptional(b.latest, a.latest) || compare(a.id, b.id)
            : value.orderby === 'r' ? compare(b.replies, a.replies) || compare(a.id, b.id)
              : compare(b.bumped, a.bumped) || compare(b.id, a.id)));
      renderedOrder = value.orderby;
    }
    if (searchReady && query !== cachedQuery) {
      cachedQuery = query;
      pattern = query ? new RegExp(query.replace(escape, '\\$1'), 'i') : null;
    }
    order.value = value.orderby;
    size.value = value.large ? 'large' : 'small';
    teaser.value = value.extended ? 'on' : 'off';
    container.className = `catalog ${value.extended ? 'extended-' : ''}${value.large ? 'large' : 'small'}`;
    const fragment = document.createDocumentFragment();
    let count = 0;
    for (const entry of entries) {
      const id = entry.id.toString();
      if (stateReady && (hiddenOnly ? !hiddenThreads.has(id) : !query && hiddenThreads.has(id))) continue;
      if (!hiddenOnly && searchReady && pattern && !entry.fields.some(field => pattern.test(field))) continue;
      if (stateReady) {
        const pinned = pins.has(id);
        entry.node.querySelector('.catalogThumb .thumb')?.classList.toggle('pinned', pinned);
        entry.pinDelta.hidden = !pinned;
        entry.pinPage.hidden = !pinned || !pages.has(id);
        if (pinned) {
          const delta = Number(entry.replies) - pins.get(id);
          entry.pinDelta.textContent = delta > 0 ? ` (+${delta})` : '(+0)';
          if (delta > 0) pins.set(id, Number(entry.replies));
        }
      }
      if (entry.thumb) {
        const [width, height] = value.large ? entry.large : entry.small;
        entry.thumb.width = width;
        entry.thumb.height = height;
      }
      if (value.extended) entry.node.append(entry.teaser);
      else entry.teaser.remove();
      fragment.append(entry.node, document.createTextNode('\n'));
      count += 1;
    }
    if (!count) {
      if (!searchReady && originalEmpty) fragment.append(originalEmpty);
      else {
        const message = document.createElement('p');
        message.className = 'empty';
        const link = document.createElement('a');
        link.href = query ? action.href : new URL('./#postForm', action).href;
        const allHidden = stateReady && !query && entries.length > 0;
        link.textContent = query ? 'Show all threads' : allHidden ? 'Show hidden threads' : 'Start the first thread';
        message.append(query ? 'No matching threads. ' : allHidden ? 'All threads are hidden. ' : 'No threads yet. ', link, '.');
        fragment.append(message);
      }
    }
    container.replaceChildren(fragment);
    if (searchReady) renderedQuery = query;
    if (stateReady) { persistState(pinKey, pins); updateStateControls(); }
    return true;
  };
  const applySearch = () => {
    clearTimeout(timer);
    if (!searchReady) return false;
    if (!validQuery(search.value)) {
      search.setCustomValidity('Search accepts up to 128 characters without control characters.');
      search.reportValidity();
      return false;
    }
    search.setCustomValidity('');
    if (!apply(current(), search.value)) return false;
    saveSearch(renderedQuery);
    updateURL(current());
    return true;
  };
  form.addEventListener('submit', event => {
    save();
    if (searchReady) { event.preventDefault(); applySearch(); }
  });
  for (const control of [order, size, teaser]) {
    control.addEventListener('change', () => {
      const value = current();
      if (!apply(value)) { form.requestSubmit(); return; }
      save();
      updateURL(value);
    });
  }
  if (searchReady) {
    container.addEventListener('click', event => {
      const link = event.target instanceof Element ? event.target.closest('.empty > a') : null;
      if (renderedQuery && link && container.contains(link)) {
        event.preventDefault();
        search.value = '';
        applySearch();
      } else if (stateReady && link && container.contains(link) && entries.length && hiddenThreads.size) {
        event.preventDefault(); hiddenOnly = true; apply(current());
      }
    });
    const schedule = () => { clearTimeout(timer); if (!composing) timer = setTimeout(applySearch, 250); };
    search.addEventListener('input', schedule);
    search.addEventListener('compositionstart', () => { composing = true; clearTimeout(timer); });
    search.addEventListener('compositionend', () => { composing = false; schedule(); });
    search.addEventListener('keydown', event => {
      if (event.key === 'Escape') { event.preventDefault(); search.value = ''; applySearch(); }
    });
  }
  reset.addEventListener('click', event => {
    clearTimeout(timer);
    try { localStorage.removeItem(key); } catch { /* Explicit defaults are also safe without removal. */ }
    saveSearch('');
    const defaults = { orderby: 'alt', large: false, extended: true };
    const url = new URL(action.href);
    setOptions(url, defaults);
    reset.href = url.href;
    if (searchReady) {
      hiddenOnly = false;
      search.value = '';
      search.setCustomValidity('');
      if (apply(defaults, '')) {
        event.preventDefault();
        try { history.replaceState(null, '', url.href); } catch { /* No history permission is required. */ }
      }
    }
  });

  if (stateReady) installThreadControls();
  const stateChanged = stateReady && (pins.size > 0 || hiddenThreads.size > 0);
  if (stateChanged) renderedOrder = null;
  const url = new URL(location.href);
  let display = current();
  if (!['order', 'size', 'teaser'].some(name => url.searchParams.has(name))) {
    try {
      const raw = localStorage.getItem(key);
      if (raw !== null && raw.length <= 1024) {
        const saved = JSON.parse(raw);
        if (valid(saved)) display = saved;
      }
    } catch { /* Ignore malformed or unavailable preferences. */ }
  }
  let query = renderedQuery;
  if (searchReady && !url.searchParams.has('q')) {
    if (url.hash.startsWith('#s=') && url.hash.length <= 2048) {
      try {
        const decoded = decodeURIComponent(url.hash.slice(3).replace(/\+/g, ' '));
        if (validQuery(decoded)) query = decoded;
      } catch { /* Malformed fragment searches leave the rendered page usable. */ }
    } else if (!url.hash) {
      try {
        const saved = sessionStorage.getItem(searchKey);
        if (saved !== null) {
          if (sessionStorage.getItem(boardKey) === board && validQuery(saved)) query = saved;
          else saveSearch('');
        }
      } catch { /* Session restoration is optional. */ }
    }
  }
  const value = current();
  const changed = display.orderby !== value.orderby || display.large !== value.large
    || display.extended !== value.extended || query !== renderedQuery || stateChanged;
  if (changed) {
    if (searchReady) search.value = query;
    if (apply(display, query)) updateURL(display);
    else {
      setOptions(url, display);
      location.replace(url.href);
    }
  }
  if (searchReady && query) saveSearch(query);
})();
