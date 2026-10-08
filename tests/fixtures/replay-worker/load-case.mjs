// Closed owned-corpus acquisition. This is not host admission or upload parsing.
import { CASES } from './cases.mjs';
const ids = new Set(CASES.map(item => item.id));
export async function loadOwnedCase(id) {
  if (!ids.has(id)) throw new Error('Unknown owned replay case');
  const manifestResponse = await fetch(new URL('./generated/manifest.json', import.meta.url));
  if (!manifestResponse.ok) throw new Error('Missing probe manifest');
  const manifest = await manifestResponse.json();
  if (manifest.schema !== 1 || manifest.consumer !== 'tegaki-worker-probe-v1') throw new Error('Unreviewed probe manifest');
  const row = manifest.cases.find(item => item.id === id);
  if (!row || row.name !== `${id}.ibr` || row.bytes > 1792 || row.events > 96) throw new Error('Invalid owned fixture envelope');
  const response = await fetch(new URL(`./generated/${row.name}`, import.meta.url));
  if (!response.ok) throw new Error('Missing owned fixture');
  // The parent deadline covers this trusted static fixture acquisition. The
  // owned static server gives fixed lengths; this is not an upload stream API.
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.length !== row.bytes) throw new Error('Owned fixture length mismatch');
  const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))].map(x => x.toString(16).padStart(2, '0')).join('');
  if (digest !== row.sha256) throw new Error('Owned fixture digest mismatch');
  return { bytes, row };
}
