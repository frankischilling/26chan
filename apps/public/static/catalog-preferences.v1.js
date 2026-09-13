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

  form.addEventListener('submit', save);
  for (const control of [order, size, teaser]) {
    control.addEventListener('change', () => form.requestSubmit());
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
  // Only finite settings enter this same-origin URL; search text is never saved.
  setOptions(url, saved);
  location.replace(url.href);
})();
