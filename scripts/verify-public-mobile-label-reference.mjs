import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import vm from 'node:vm';
import { parseFragment } from 'parse5';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(args.length === 1 || args.length === 2 && args[1] === '--write', 'Use <extension.1191.js> [--write]');
const release = JSON.parse(await readFile(new URL('docs/public-watcher-assets.json', root), 'utf8'));
const source = await readFile(args[0]);
assert.equal(createHash('sha256').update(source).digest('hex'), release.source_sha256);
const text = source.toString('utf8'), context = vm.createContext({ Parser: {} }), helpers = [];
for (const [name, bytes] of [['decodeSpecialChars', 157], ['encodeSpecialChars', 157], ['truncate', 139]]) {
  const marker = `Parser.${name}=function`;
  assert.equal(text.split(marker).length, 2);
  const start = text.indexOf(marker), end = text.indexOf(',Parser.', start);
  assert.ok(end > start);
  const helper = text.slice(start, end);
  assert.equal(helper.length, bytes);
  new vm.Script(helper).runInContext(context, { timeout: 1000 });
  helpers.push({ name, bytes });
}
const cases = [];
for (const [name, input] of [
  ['thirty ASCII', 'A'.repeat(30)], ['thirty-one ASCII', 'A'.repeat(31)],
  ['entity-expanded short label', '<'.repeat(10)], ['ampersands', '&'.repeat(10)],
  ['quotes and apostrophes', '"\'<> &'.repeat(5)],
  ['split surrogate boundary', 'A'.repeat(29) + '😀'], ['complete surrogate boundary', 'A'.repeat(28) + '😀B'],
  ['combining mark boundary', 'e\u0301'.repeat(16)], ['line break and spaces', 'Owned\n name  ' + 'A'.repeat(25)],
  ['encoded-looking user text', '&amp;lt;'.repeat(6)],
]) {
  const serialized = context.Parser.encodeSpecialChars(input), shortened = serialized.length > 30;
  const output = shortened ? context.Parser.truncate(serialized, 30) + '(...)' : serialized;
  // JSON/Rust strings cannot carry an unpaired surrogate. Preserve exact source
  // UTF-16 units separately, and record the valid-UTF-8 HTML rendering.
  const sourceUnits = Array.from({ length: output.length }, (_, index) => output.charCodeAt(index));
  const encodedResult = Buffer.from(output, 'utf8').toString('utf8');
  const fragment = parseFragment(encodedResult);
  assert.ok(fragment.childNodes.every(node => node.nodeName === '#text'));
  const visible = fragment.childNodes.map(node => node.value).join('');
  cases.push({ name, input, serialized, serializedUnits: serialized.length, shortened, sourceUnits, encodedResult, visible });
}
const comma = context.Parser.truncate('&#44;'.repeat(8), 30);
const result = {
  scope: 'Released pure-helper vectors on explicitly HTML-escaped synthetic text and the formatter condition. Exact UTF-16 units are retained when a surrogate is split; visible strings use valid UTF-8. This does not establish original-server name serialization, accepted input or normalization.',
  client: { url: release.source, sha256: release.source_sha256 }, helpers, cases,
  serializedCommaControl: { input: '&#44;'.repeat(8), result: comma },
};
const target = new URL('docs/public-mobile-label-reference.json', root);
if (args[1] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
else assert.deepEqual(result, JSON.parse(await readFile(target, 'utf8')));
console.log(`Verified ${cases.length} mobile label vectors and the serialized comma control without initialization or network access.`);
