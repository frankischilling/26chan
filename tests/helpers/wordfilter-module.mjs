import { readFile } from 'node:fs/promises';

// Blob/data modules have no hierarchical base for relative imports. Include
// the production recipe dependency without changing either module's behavior.
export async function wordfilterModuleURL() {
  const base = new URL('../../apps/public/client/', import.meta.url);
  const data = source => `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`;
  const exif = data(await readFile(new URL('native-exif.js', base), 'utf8'));
  const source = await readFile(new URL('native-wordfilter-markup.js', base), 'utf8');
  return data(source.replace('./native-exif.js', exif));
}
