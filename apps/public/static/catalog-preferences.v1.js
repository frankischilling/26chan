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
    if (value.orderby !== renderedOrder) {
      entries.sort((a, b) => Number(b.sticky) - Number(a.sticky) || (
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
      if (searchReady && pattern && !entry.fields.some(field => pattern.test(field))) continue;
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
        link.textContent = query ? 'Show all threads' : 'Start the first thread';
        message.append(query ? 'No matching threads. ' : 'No threads yet. ', link, '.');
        fragment.append(message);
      }
    }
    container.replaceChildren(fragment);
    if (searchReady) renderedQuery = query;
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
      search.value = '';
      search.setCustomValidity('');
      if (apply(defaults, '')) {
        event.preventDefault();
        try { history.replaceState(null, '', url.href); } catch { /* No history permission is required. */ }
      }
    }
  });

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
    || display.extended !== value.extended || query !== renderedQuery;
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
