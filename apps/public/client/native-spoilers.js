import { isSpoilerAssetPath } from './native-spoiler-assets.js';

// Each browser bundle has its own module scope. A document owns the source's
// one-choice-per-board cache, including choices first made by another bundle.
const cacheKey = Symbol.for('26chan.native.custom-spoilers');
function state(document) {
  if (!document[cacheKey]) Object.defineProperty(document, cacheKey, {
    value: { choices: new Map(), reveal: () => false },
  });
  return document[cacheKey];
}
export function configureSpoilerPreference(document, reveal) {
  if (typeof reveal !== 'function') throw new TypeError('spoiler-preference');
  state(document).reveal = reveal;
}
export function sourceSpoilerPath(document, board, count = 0) {
  if (typeof board !== 'string' || !/^[a-z0-9]{1,10}$/.test(board)
    || !Number.isInteger(count) || count < 0 || count > 64) throw new TypeError('spoiler-policy');
  const cache = state(document);
  const existing = cache.choices.get(board);
  if (existing) {
    if (!isSpoilerAssetPath(existing)) throw new TypeError('spoiler-cache');
    return existing;
  }
  if (!count || cache.reveal() === true) return '/static/catalog/spoiler.png';
  if (cache.choices.size >= 100) throw new RangeError('spoiler-boards');
  const current = document.getElementById?.('watcher-context')?.getAttribute('data-board');
  const primary = current === board ? document.querySelector?.('.imgspoiler img')?.getAttribute('src') : null;
  const path = isSpoilerAssetPath(primary) && primary !== '/static/catalog/spoiler.png'
    ? primary : `/static/catalog/spoiler-${board}${Math.floor(Math.random() * count) + 1}.png`;
  if (!isSpoilerAssetPath(path)) throw new TypeError('unavailable-spoiler-policy');
  cache.choices.set(board, path);
  return path;
}
