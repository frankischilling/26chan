import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import vm from 'node:vm';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(args.length === 2 || args.length === 3 && args[2] === '--write', 'Use <extension.1191.js> <core.1128.js> [--write]');
const extensionManifest = JSON.parse(await readFile(new URL('docs/public-watcher-assets.json', root), 'utf8'));
const coreManifest = JSON.parse(await readFile(new URL('docs/public-theme-reference.json', root), 'utf8')).client_asset;
const texts = [];
for (const [path, hash] of [[args[0], extensionManifest.source_sha256], [args[1], coreManifest.sha256]]) {
  const bytes = await readFile(path);
  assert.equal(createHash('sha256').update(bytes).digest('hex'), hash);
  texts.push(bytes.toString('utf8'));
}
const [extension, core] = texts;
function extract(text, start, end, length) {
  assert.equal(text.split(start).length, 2);
  const offset = text.indexOf(start), stop = text.indexOf(end, offset);
  assert.ok(stop > offset);
  const body = text.slice(offset, stop);
  assert.equal(Buffer.byteLength(body), length);
  return body;
}
function parserFunction(name, length) {
  const marker = `Parser.${name}=function`;
  assert.equal(extension.split(marker).length, 2);
  const offset = extension.indexOf(marker);
  const next = /,Parser\.[A-Za-z0-9_]+=function/.exec(extension.slice(offset + marker.length));
  assert.ok(next);
  const body = extension.slice(offset, offset + marker.length + next.index);
  assert.equal(Buffer.byteLength(body), length);
  return body;
}
const helpers = [
  ['age', extract(extension, '$.ago=function', ',$.hash=function', 350)],
  ['full-label callback', extract(core, 'function mShowFull(e)', 'function loadBannerImage()', 460)],
  ['Tip literal', extract(core, 'Tip={node:null', ';captchainterval=null', 1168)],
  ['date hover', parserFunction('onDateMouseOver', 191)],
  ['date hover cancellation', parserFunction('onTipMouseOut', 108)],
];
const timestamp = 1801324800;
let now = timestamp * 1000;
const scheduled = new Map(), records = [], cancelled = [];
let nextTimer = 0;
const context = vm.createContext({
  $: {}, Parser: { tipTimeout: null }, Date: { now: () => now },
  setTimeout(callback, delay, element, text) {
    assert.equal(callback, context.Tip.show);
    const id = ++nextTimer;
    scheduled.set(id, { delay, element, text }); records.push({ delay, text });
    return id;
  },
  clearTimeout(id) { assert.ok(scheduled.delete(id)); cancelled.push(id); },
});
for (const [name, body] of helpers) {
  new vm.Script(name === 'Tip literal' ? `var ${body};` : body).runInContext(context, { timeout: 1000 });
}
const ages = [];
for (const delta of [-1, 0, 0.5, 1, 2, 59.9, 60, 61, 119, 120, 3599, 3600, 3660, 3720, 86399, 86400, 90000, 93600, 172800]) {
  now = timestamp * 1000 + delta * 1000;
  ages.push({ delta, text: context.$.ago(timestamp) });
}
now = timestamp * 1000;
context.Parser.onDateMouseOver({ getAttribute: () => String(timestamp - 30) });
context.Parser.onDateMouseOver({ getAttribute: () => String(timestamp - 90) });
assert.equal(scheduled.size, 1);
context.Parser.onTipMouseOut();
assert.equal(scheduled.size, 0);
assert.equal(context.Parser.tipTimeout, null);

const fullHTML = 'Owned &lt;label&gt; &amp; text';
const ancestor = { getElementsByClassName: () => [{ innerHTML: 'Owned shortened' }, { innerHTML: fullHTML }] };
const name = { className: 'name', parentNode: { parentNode: { parentNode: ancestor } } };
const names = [
  { kind: 'exact ordinary name', result: context.mShowFull(name) ?? null },
  { kind: 'additional class', result: context.mShowFull({ ...name, className: 'name extra' }) ?? null },
];
const subjects = [
  { kind: 'formatter direct subject child', result: context.mShowFull({ className: 'subject', parentNode: { className: 'nameBlock' } }) ?? null },
  { kind: 'wrapped subject control', result: context.mShowFull({ className: 'owned', parentNode: { className: 'subject', parentNode: { parentNode: { parentNode: ancestor } } } }) ?? null },
];
const filenames = [];
for (const [kind, title, html] of [
  ['title filename', 'Owned full filename.png', 'Owned short.png'],
  ['HTML-looking title', '<img src="https://invalid.example/owned-tooltip-control">.png', 'Owned short.png'],
  ['missing title fallback', null, 'Owned &lt;file&gt;.png'],
]) {
  const element = { className: 'mFileInfo mobile', parentNode: { className: 'fileThumb', parentNode: {
    getElementsByClassName: () => [{ firstElementChild: { getAttribute: () => title, innerHTML: html } }],
  } } };
  filenames.push({ kind, title, html, result: context.mShowFull(element) ?? null });
}
const result = {
  collection_date: '2026-09-30',
  scope: 'Pure released age and callback vectors on synthetic values, plus virtual timer cancellation. No extension initialization, original-server DOM, tooltip HTML construction, external request or full-page pixels qualified. The formatter-shaped direct subject returns no callback value; the wrapped control does not establish original-server markup.',
  clients: [
    { url: extensionManifest.source, sha256: extensionManifest.source_sha256 },
    { url: coreManifest.url, sha256: coreManifest.sha256 },
  ],
  helpers: helpers.map(([name, body]) => ({ name, bytes: Buffer.byteLength(body), sha256: createHash('sha256').update(body).digest('hex') })),
  coreDelay: context.Tip.delay,
  dateHover: { records, cancelled, remaining: scheduled.size },
  ages, names, subjects, filenames,
};
const target = new URL('docs/public-post-tooltip-reference.json', root);
if (args[2] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target, 'utf8')));
console.log(`Verified ${ages.length} age boundaries, bounded synthetic label/file callbacks and date timer cancellation without initialization or network access.`);
