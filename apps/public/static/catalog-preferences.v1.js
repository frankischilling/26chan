(() => {
  'use strict';
  const form = document.getElementById('ctrl');
  const order = document.getElementById('order-ctrl');
  const size = document.getElementById('size-ctrl');
  const teaser = document.getElementById('teaser-ctrl');
  const reset = document.getElementById('catalog-reset');
  if (!(form instanceof HTMLFormElement) || !(order instanceof HTMLSelectElement)
      || !(size instanceof HTMLSelectElement) || !(teaser instanceof HTMLSelectElement)
      || !(reset instanceof HTMLAnchorElement)) return;

  const key = 'catalog-settings';
  const orders = ['alt', 'absdate', 'date', 'r'];
  const current = () => ({ orderby: order.value, large: size.value === 'large', extended: teaser.value === 'on' });
  const valid = value => value !== null && typeof value === 'object' && !Array.isArray(value)
    && orders.includes(value.orderby) && typeof value.large === 'boolean' && typeof value.extended === 'boolean';
  const container = document.getElementById('threads');
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
  try {
    if (!(container instanceof HTMLElement)) throw new Error('Missing catalog');
    entries = Array.from(container.querySelectorAll(':scope > .thread'), node => {
      const data = node.dataset;
      if (!['true', 'false'].includes(data.sticky)) throw new Error('Invalid sticky flag');
      const teaserNode = node.querySelector(':scope > .teaser')
        ?? node.querySelector(':scope > template.catalogTeaser')?.content.querySelector('.teaser');
      if (!(teaserNode instanceof HTMLElement)) throw new Error('Missing teaser');
      const thumb = node.querySelector('.catalogThumb img[id^="thumb-"]');
      return { node, teaser: teaserNode, thumb,
        small: thumb ? dimensions(thumb, 'small', 150) : null,
        large: thumb ? dimensions(thumb, 'large', 250) : null,
        id: integer(data.threadId), bumped: integer(data.bumped, true),
        latest: data.latestReply === '' ? null : integer(data.latestReply), replies: integer(data.replies), sticky: data.sticky === 'true' };
    });
  } catch { entries = null; }
  let renderedOrder = order.value;
  const compare = (a, b) => a < b ? -1 : a > b ? 1 : 0;
  const compareOptional = (a, b) => a === null ? (b === null ? 0 : -1) : b === null ? 1 : compare(a, b);
  const apply = value => {
    if (entries === null || !valid(value)) return false;
    if (value.orderby !== renderedOrder) {
      entries.sort((a, b) => Number(b.sticky) - Number(a.sticky) || (
        value.orderby === 'date' ? compare(b.id, a.id)
          : value.orderby === 'absdate' ? compareOptional(b.latest, a.latest) || compare(a.id, b.id)
            : value.orderby === 'r' ? compare(b.replies, a.replies) || compare(a.id, b.id)
              : compare(b.bumped, a.bumped) || compare(b.id, a.id)));
      renderedOrder = value.orderby;
    }
    order.value = value.orderby;
    size.value = value.large ? 'large' : 'small';
    teaser.value = value.extended ? 'on' : 'off';
    container.className = `catalog ${value.extended ? 'extended-' : ''}${value.large ? 'large' : 'small'}`;
    const fragment = document.createDocumentFragment();
    for (const entry of entries) {
      if (entry.thumb) {
        const [width, height] = value.large ? entry.large : entry.small;
        entry.thumb.width = width;
        entry.thumb.height = height;
      }
      if (value.extended) entry.node.append(entry.teaser);
      else entry.teaser.remove();
      // Keep both the actual card nodes and the inline-block whitespace gaps.
      fragment.append(entry.node, document.createTextNode('\n'));
    }
    if (entries.length > 0) container.replaceChildren(fragment);
    return true;
  };
  const save = () => {
    const value = current();
    if (!valid(value)) return;
    try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* Browsing still works without storage. */ }
  };
  const setOptions = (url, value) => {
    url.searchParams.set('order', value.orderby);
    url.searchParams.set('size', value.large ? 'large' : 'small');
    url.searchParams.set('teaser', value.extended ? 'on' : 'off');
  };
  const updateURL = value => {
    const url = new URL(location.href);
    setOptions(url, value);
    try { history.replaceState(null, '', url.href); } catch { /* Display controls still work if history is unavailable. */ }
  };

  form.addEventListener('submit', save);
  for (const control of [order, size, teaser]) {
    control.addEventListener('change', () => {
      const value = current();
      if (!apply(value)) { form.requestSubmit(); return; }
      save();
      updateURL(value);
    });
  }
  reset.addEventListener('click', () => {
    try { localStorage.removeItem(key); } catch { /* Explicit defaults also work if removal is denied. */ }
    const url = new URL(location.href);
    url.search = '';
    url.hash = '';
    setOptions(url, { orderby: 'alt', large: false, extended: true });
    reset.href = url.href;
  });

  // Explicit URLs are authoritative and do not silently overwrite preferences.
  const url = new URL(location.href);
  if (['order', 'size', 'teaser'].some(name => url.searchParams.has(name))) return;
  let saved;
  try {
    const raw = localStorage.getItem(key);
    if (raw === null || raw.length > 1024) return;
    saved = JSON.parse(raw);
  } catch { return; }
  if (!valid(saved)) return;
  const value = current();
  if (saved.orderby === value.orderby && saved.large === value.large && saved.extended === value.extended) return;
  if (apply(saved)) { updateURL(saved); return; }
  // Only finite settings enter this same-origin URL; search text is never saved.
  setOptions(url, saved);
  location.replace(url.href);
})();
