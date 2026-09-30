import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import vm from 'node:vm';
import { parseFragment } from 'parse5';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <extension.1191.js> [--write]');
const pin = JSON.parse(await readFile(new URL('docs/public-watcher-assets.json', root)));
const source = await readFile(args[0]);
assert.equal(createHash('sha256').update(source).digest('hex'), pin.source_sha256);
const text = source.toString('utf8');
const marker = 'Parser.buildHTMLFromJSON=', start = text.indexOf(marker) + marker.length;
assert.equal(text.split(marker).length, 2);
const formatter = text.slice(start, text.indexOf(',Parser.truncate=function', start));
assert.equal(Buffer.byteLength(formatter), 6986);
const context = vm.createContext({
  Main: { board: 'demo', tid: 1, hasMobileLayout: true }, Config: { revealSpoilers: false },
  $L: { d: () => 'reference.invalid' }, Parser: { icons: {}, customSpoiler: {} },
  document: { createElement: () => ({ getElementsByClassName: () => [] }) },
});
const helpers = [];
for (const name of ['decodeSpecialChars', 'encodeSpecialChars']) {
  const marker = `Parser.${name}=function`, start = text.indexOf(marker);
  assert.equal(text.split(marker).length, 2);
  const next = /,Parser\.[A-Za-z0-9_]+=function/.exec(text.slice(start + marker.length));
  assert.ok(next);
  const body = text.slice(start, start + marker.length + next.index);
  helpers.push({ name, sha256: createHash('sha256').update(body).digest('hex') });
  new vm.Script(body).runInContext(context, { timeout: 1000 });
}
const build = new vm.Script(`(${formatter})`).runInContext(context, { timeout: 1000 });
const encode = context.Parser.encodeSpecialChars;
function find(tree, predicate) {
  if (predicate(tree)) return tree;
  for (const child of tree.childNodes ?? []) { const found = find(child, predicate); if (found) return found; }
}
const content = node => node?.nodeName === '#text' ? node.value : (node?.childNodes ?? []).map(content).join('');
const attr = (node, name) => node?.attrs?.find(row => row.name === name)?.value ?? null;
const cases = [];
const names = ['fold', 'a'.repeat(30), 'a'.repeat(31), 'a'.repeat(40), 'a'.repeat(41),
  '<fold> & "roof"', 'literal &lt;fold&gt;', 'λ'.repeat(41), 'a'.repeat(24) + '😀'.repeat(9)];
const inputs = names.map(filename => ({ filename, bytes: 2048 }));
for (const bytes of [0, 1, 1024, 1025, 1535, 1536, 1048575, 1048576, 1053818, 1053819, 33554432]) inputs.push({ filename: 'fold', bytes });
for (const kind of ['op', 'reply']) for (const input of inputs) {
  const no = kind === 'op' ? 1001001 : 1001002;
  const rendered = build({ no, resto: kind === 'op' ? 0 : 1001001, name: 'Owned', now: 'fixed', time: 1788868800,
    filename: encode(input.filename), ext: '.png', fsize: input.bytes, w: 600, h: 360,
    tn_w: 250, tn_h: 150, tim: no, md5: 'owned' }, 'demo', false, false);
  const tree = parseFragment(rendered.innerHTML);
  const fileText = find(tree, node => attr(node, 'class') === 'fileText');
  const link = find(fileText, node => node.tagName === 'a');
  const mobile = find(tree, node => attr(node, 'class') === 'mFileInfo mobile');
  const label = content(link);
  cases.push({ kind, ...input, label_units: Array.from({ length: label.length }, (_, i) => label.charCodeAt(i)),
    label: label.toWellFormed(), title: attr(link, 'title'), file_text: content(fileText).toWellFormed(),
    mobile_info: content(mobile), file_text_id: attr(fileText, 'id') });
}
const result = { collection_date: '2026-09-30', source: pin.source, sha256: pin.source_sha256, helpers,
  scope: 'Isolated released formatter with escaped synthetic filenames and bounded numeric metadata. No extension initialization, external requests, original user content, server filename acceptance or full-page pixels.', cases };
const target = new URL('docs/public-file-reference.json', root);
if (args[1] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target, 'utf8')));
console.log(`Verified ${cases.length} released file labels, unit boundaries and mobile captions.`);
