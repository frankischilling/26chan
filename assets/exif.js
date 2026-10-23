// Release-owned EXIF toggles. No inline handlers, HTML parsing or remote reads.
const mounts = new WeakMap();
export function exifTarget(value) {
  if (typeof value !== 'string' || !/^[e3]xif[1-9][0-9]{0,15}$/.test(value)) return null;
  return BigInt(value.slice(4)) <= 9007199254740991n ? value : null;
}
export function mountExif(doc = globalThis.document) {
  if (!doc?.addEventListener) return null;
  mounts.get(doc)?.destroy();
  const click = event => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const link = event.target?.closest?.('a[data-exif-toggle]');
    if (!link || !doc.contains(link)) return;
    const id = exifTarget(link.getAttribute('data-exif-toggle'));
    const scope = link.closest('blockquote');
    if (!id || !scope) return;
    const table = scope.querySelector(`table[data-exif-table="${id}"]`);
    if (!table || table.id !== id) return;
    event.preventDefault();
    const expanded = table.getAttribute('data-exif-expanded') !== 'true';
    table.setAttribute('data-exif-expanded', String(expanded));
    link.setAttribute('aria-expanded', String(expanded));
  };
  doc.addEventListener('click', click);
  const api = { destroy() { doc.removeEventListener('click', click); if (mounts.get(doc) === api) mounts.delete(doc); } };
  mounts.set(doc, api);
  return api;
}
if (typeof document !== 'undefined') mountExif(document);
