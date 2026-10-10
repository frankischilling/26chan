import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';

const args = process.argv.slice(2);
assert.ok(args[0] === '--source' && args[1]
  && (args.length === 2 || (args.length === 3 && args[2] === '--check')),
'Usage: node scripts/record-drawing-edit-source.mjs --source /path/to/4chan-old [--check]');
const target = new URL('../tests/fixtures/native-drawing-source.json', import.meta.url);
const oracle = JSON.parse(await readFile(target, 'utf8'));
const file = 'js/extension.js';
const bytes = await readFile(resolve(args[1], file));
const hash = value => createHash('sha256').update(value).digest('hex');
assert.equal(hash(bytes), oracle.files[file], 'The supplied source file differs from the pinned revision');
for (const [name, start, end] of [
  ['edit_board', 'Main.isOekakiBoard =', '\r\n'],
  ['edit_gate', 'if (Main.isOekakiBoard && Main.tid) {', '\r\n  }\r\n'],
  ['edit_link', 'Parser.addOekakiEditLink = function', 'Parser.getLocaleDate = function'],
  ['qr_edit', 'QR.onOpenInPainterClick = function', 'QR.openPainter = function'],
]) {
  const byte_start = bytes.indexOf(start);
  assert.ok(byte_start >= 0 && bytes.indexOf(start, byte_start + start.length) < 0);
  const byte_end = bytes.indexOf(end, byte_start + start.length);
  assert.ok(byte_end > byte_start);
  const source = bytes.subarray(byte_start, byte_end);
  const row = { file, byte_start, byte_end, sha256: hash(source), text: source.toString('utf8') };
  assert.equal(Buffer.byteLength(row.text), source.length);
  if (args.includes('--check')) assert.deepEqual(oracle.snippets[name], row);
  else oracle.snippets[name] = row;
}
if (!args.includes('--check')) await writeFile(target, JSON.stringify(oracle, null, 2) + '\n');
console.log('Drawing Edit references match the pinned original source.');
