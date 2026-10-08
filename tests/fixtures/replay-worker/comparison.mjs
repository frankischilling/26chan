// Shared assertion plumbing only. No decoder, source preparation or raster logic.
export function mismatch(a, b, path = 'root') {
  if (Object.is(a, b)) return null;
  if (!a || !b || typeof a !== 'object' || typeof b !== 'object') return path;
  const ak = Object.keys(a), bk = Object.keys(b);
  if (ak.length !== bk.length || ak.some((key, i) => key !== bk[i])) return `${path}.keys`;
  for (const key of ak) { const result = mismatch(a[key], b[key], `${path}.${key}`); if (result) return result; }
  return null;
}
export function equal(a, b, label) {
  const path = mismatch(a, b); if (path) throw new Error(`${label} mismatch: ${path}`);
}
export function compareObservation(actual, expected, flattened, index) {
  // These remain distinct even though authoritative data also occurs in state.
  equal(actual.state.layers.map(layer => layer.authoritative), expected.layers.map(layer => layer.authoritative), `authoritative at ${index}`);
  equal(actual.state.layers.map(layer => layer.nativeReadback), expected.layers.map(layer => layer.nativeReadback), `native readback at ${index}`);
  equal(actual.state, expected, `core state at ${index}`);
  equal(actual.flattened, flattened, `source flatten at ${index}`);
  equal(actual.presented, flattened, `received bitmap at ${index}`);
}
const channels = Object.freeze({
  authoritative: { prefix: 'authoritative at ', bytes: value => value.state.layers[0].authoritative },
  nativeReadback: { prefix: 'native readback at ', bytes: value => value.state.layers[0].nativeReadback },
  sourceFlatten: { prefix: 'source flatten at ', bytes: value => value.flattened },
  receivedBitmap: { prefix: 'received bitmap at ', bytes: value => value.presented },
});
export const COMPARISON_CHANNELS = Object.freeze(Object.keys(channels));
export function comparisonNegative(actual, expected, flattened, index, channel) {
  const control = channels[channel];
  if (!control) throw new Error('Unknown comparison negative control');
  compareObservation(actual, expected, flattened, index); // establish positive baseline first
  const changed = structuredClone(actual), bytes = control.bytes(changed);
  if (!Array.isArray(bytes) || bytes.length === 0) throw new Error('Missing byte channel');
  bytes[0] ^= 1; // Only a copied diagnostic byte changes; no live raster is edited.
  let rejection;
  try { compareObservation(changed, expected, flattened, index); }
  catch (error) { rejection = error.message; }
  if (!rejection?.startsWith(control.prefix)) throw new Error(`Negative control escaped its ${channel} comparator: ${rejection || 'accepted'}`);
  compareObservation(actual, expected, flattened, index); // original observations remain intact
  return { channel, rejection, changedBytes: 1, originalUnchanged: true };
}
