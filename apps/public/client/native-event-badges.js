// Finite source identity classes. No caller-defined stock names or image URLs.
const stocks = new Set(['PEPE', 'WOJK', 'ANIME', 'CHAD', 'CLOWN', 'LOL', 'SICP', 'AUTSM', 'BANE', 'CIA', 'BOOB', 'RDDT', 'DESU', 'JANNY', 'GME', 'CHUCK', 'YTSB', 'GACHI']);
export function stockToken(value) {
  return typeof value === 'string' && (value === 'n-xa24' || /^(?:n|p)-xa24-/.test(value) && stocks.has(value.slice(7)));
}
export function stockBadge(value) {
  if (typeof value !== 'string' || !value.startsWith('n-xa24 n-xa24-')) return null;
  const stock = value.slice(14);
  return stocks.has(stock) ? stock : null;
}
export function stockContainer(value, base) {
  if (value === base) return null;
  if (typeof value !== 'string' || !value.startsWith(`${base} p-xa24-`)) return undefined;
  const stock = value.slice(base.length + 8);
  return stocks.has(stock) ? stock : undefined;
}
