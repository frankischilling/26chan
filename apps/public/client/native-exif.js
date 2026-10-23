// The media parser supplies text only. These recipes describe release-owned DOM.
export const exifTags = new Set(['table', 'tbody', 'tr', 'td']);
const id = value => typeof value === 'string' && /^exif[1-9][0-9]{0,15}$/.test(value)
  && BigInt(value.slice(4)) <= 9007199254740991n;
export function exifRecipe(tag, attrs) {
  const keys = Object.keys(attrs).sort().join(',');
  if (tag === 'span' && keys === 'class' && attrs.class === 'abbr') return { kind: 'label' };
  if (tag === 'table' && (keys === 'class,data-exif-table,id' || (keys === 'class,data-exif-expanded,data-exif-table,id' && ['true', 'false'].includes(attrs['data-exif-expanded']))) && attrs.class === 'exif'
    && id(attrs.id) && attrs['data-exif-table'] === attrs.id) return { kind: 'table', id: attrs.id };
  if (tag === 'a' && keys === 'aria-expanded,data-exif-toggle,href' && ['true', 'false'].includes(attrs['aria-expanded'])
    && id(attrs['data-exif-toggle']) && attrs.href === `#${attrs['data-exif-toggle']}`) return { kind: 'toggle', id: attrs['data-exif-toggle'] };
  if (['tbody', 'tr'].includes(tag) && keys === '') return { kind: tag };
  if (tag === 'td' && (keys === '' || (keys === 'colspan' && attrs.colspan === '2'))) return { kind: 'cell' };
  return null;
}
