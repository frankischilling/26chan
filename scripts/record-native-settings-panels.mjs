import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

const root = new URL('../', import.meta.url);
export const sourceHash = '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31';
const hashes = {
  desktop: 'd54fe5043e59573bb6f398ee3cbacb0309b569aca556a8fc2bcd2575de2fae48',
  mobileMedia: '63af4f7d7588ad61ac3bbd516e4b2fdb0b62ebf1c73a7a86c93162697b8da65e',
  mobilePanel: 'a9da23369844d39ca909c9496c996ebbfd6b85c9fbb5c238c9deef7d1ba0b653',
  keybinds: '13fc7a08fcec3ed18b9bff3b7c264c6c5e632810f2097f43b67cc54a029c99b6',
};
const hash = value => createHash('sha256').update(value).digest('hex');
export function extractSource(bytes) {
  assert.equal(hash(bytes), sourceHash, 'Unexpected pinned source bytes');
  const source = bytes.toString('utf8').replaceAll('\r\n', '\n');
  const section = (start, end, from = 0) => {
    const at = source.indexOf(start, from), until = source.indexOf(end, at + start.length);
    assert.ok(at >= 0 && until > at, `Missing source markers: ${start}`);
    return source.slice(at, until);
  };
  const media = '@media only screen and (max-width: 480px) {\\\n';
  assert.ok(source.includes(media), 'Missing mobile media wrapper');
  return {
    desktop: section('.UIMenu,\\', '#settingsMenu label input'),
    mobileMedia: media,
    mobilePanel: section('.UIPanel > div {\\', '.UIPanel .export-field', source.indexOf(media)),
    keybinds: section('Keybinds.open = function() {', 'Keybinds.close = function() {'),
  };
}
export function buildReference(snippets) {
  assert.deepEqual(Object.keys(snippets).sort(), Object.keys(hashes).sort());
  for (const [name, expected] of Object.entries(hashes)) assert.equal(hash(snippets[name]), expected, `Changed source snippet: ${name}`);
  const unescapeCSS = raw => raw.replaceAll('\\\n', '\n');
  const shortcuts = [...snippets.keybinds.matchAll(/<li><kbd>([^<]+)<\/kbd>([^<]*)<\/li>/g)]
    .map(([, key, text]) => key + text.replaceAll('&mdash;', '\u2014'));
  assert.equal(shortcuts.length, 12);
  return {
    source: 'js/extension.js', revision: '545b7812d1849f7958d914950c91fdbbe38f6b22', sha256: sourceHash,
    desktopLines: '10503-10567', mobileLines: '11063,11157-11160', keybindsLines: '8304-8340',
    css: unescapeCSS(snippets.desktop + '\n' + snippets.mobileMedia + snippets.mobilePanel) + '}\n',
    shortcuts,
    families: { yotsuba: 'futaba', 'yotsuba-b': 'burichan', futaba: 'futaba', burichan: 'burichan', tomorrow: 'tomorrow', photon: 'photon' },
  };
}
async function main() {
  const args = process.argv.slice(2);
  const check = args[0] === '--check';
  if (check) args.shift();
  let source;
  if (args.length) {
    assert.equal(args.shift(), '--source');
    source = args.shift(); assert.ok(source); assert.equal(args.length, 0);
  }
  const retained = JSON.parse(await readFile(new URL('tests/fixtures/native-settings-panel-snippets.json', root), 'utf8'));
  const snippets = source ? extractSource(await readFile(source)) : retained;
  assert.deepEqual(snippets, retained, 'Retained snippets differ from pinned full source');
  const result = JSON.stringify(buildReference(snippets), null, 2) + '\n';
  const target = new URL('tests/fixtures/native-settings-panel-source.json', root);
  if (check) assert.equal(await readFile(target, 'utf8'), result, 'Source-derived panel fixture is stale');
  else await writeFile(target, result);
  console.log(`Pinned Settings panel CSS and shortcut fixture ${check ? 'verified' : 'written'}.`);
}
if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) await main();
