import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

assert.equal(process.argv.length, 3, 'usage: node scripts/verify-public-catalog-search.mjs <pinned-asset-directory>');
const reference = JSON.parse(await readFile(new URL('../docs/public-catalog-reference.json', import.meta.url), 'utf8'));
const contract = JSON.parse(await readFile(new URL('../tests/fixtures/catalog-search-cases.json', import.meta.url), 'utf8'));
const pin = reference.assets.find(asset => asset.path === 'js/catalog.min.1025.js');
const bytes = await readFile(resolve(process.argv[2], 'catalog.min.1025.js'));
assert.equal(bytes.length, pin.bytes);
assert.equal(createHash('sha256').update(bytes).digest('hex'), pin.sha256);
const source = bytes.toString('utf8');
const start = source.indexOf('function o(){');
const end = source.indexOf('}function s(', start);
assert(start >= 0 && end > start);
const factory = source.slice(start, end + 1);
// Read only the JSON string array in the inspected factory, never eval the client.
const array = factory.match(/\[\s*"(?:\\.|[^"\\])*"(?:\s*,\s*"(?:\\.|[^"\\])*")*\s*\]/);
assert(array);
const characters = JSON.parse(array[0]);
assert.deepEqual(characters, contract.escape_characters);
assert(source.includes('Ze=new RegExp(t,"i")'));
assert(source.includes('c(250,p)'));
const escape = new RegExp('(' + characters.map(character => '\\' + character).join('|') + ')', 'g');
for (const entry of contract.cases) {
  const pattern = new RegExp(entry.query.replace(escape, '\\$1'), contract.flags);
  assert.equal(pattern.test(entry.text), entry.matches, JSON.stringify(entry));
}
console.log(`PASS ${contract.cases.length} catalog search cases; pinned client bytes and escape list verified, no full-client execution`);
