import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';

const target = new URL('../fixtures/source-quote-spellings.json', import.meta.url);
const sourceFile = new URL('../4chan-old/js/extension.js', import.meta.url);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const sourceHash = '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31';
const names = ['Parser.parseBacklinks', 'Parser.buildPost', 'QuoteInline.isSelfQuote', 'QuotePreview.init', 'QuotePreview.resolve'];
const hashes = [
  '19ab5c7e24c512d7cbc6010d571bf52a27927e267066b9ec002ed3ffcfab4259',
  '9d06076ea1a1f9f9866b7d0bd9352361c0e038ec703f9bf782205548d32852c0',
  '09a9f5ebb10ea1c46c5657e472ed73c2fb5efbc7903f1324af67748f4c1acec9',
  'e03d19108e334c7860d0d72a3678b669a0a538799b14f8357dac1bc37eccc931',
  '1e472a405e3e10cb88f7ce751c2e143e5169a4d4aeb1e13135333bc030ed36a8',
];
const args = process.argv.slice(2);
assert.ok(args.length === 1 && ['--write', '--check'].includes(args[0]));
let raw;
try { raw = await readFile(sourceFile); }
catch (error) {
  if (args[0] !== '--check' || error.code !== 'ENOENT') throw error;
  const fixture = JSON.parse(await readFile(target, 'utf8'));
  assert.equal(fixture.source_sha256, sourceHash);
  assert.equal(fixture.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
  assert.deepEqual(Object.keys(fixture.excerpts), names);
  names.forEach((name, index) => assert.equal(sha(fixture.excerpts[name].text), hashes[index]));
  console.log('Five captured source quote functions match their pinned hashes; original tree is absent.');
  process.exit(0);
}
assert.equal(sha(raw), sourceHash, 'Original extension source drift');
const source = raw.toString('utf8');
const excerpts = Object.fromEntries(names.map((name, index) => {
  const start = source.indexOf(`${name} = function(`);
  assert.ok(start >= 0 && source.indexOf(`${name} = function(`, start + 1) === -1);
  const end = source.indexOf('\n};', start) + 4;
  assert.ok(end > start);
  const text = source.slice(start, end);
  assert.equal(sha(text), hashes[index], `Quote source function drift: ${name}`);
  return [name, { sha256: sha(text), text }];
}));
const fixture = {
  source_revision: '545b7812d1849f7958d914950c91fdbbe38f6b22', source_file: 'js/extension.js', source_sha256: sourceHash,
  limits: ['Only these extracted functions may execute in isolated synthetic test contexts.',
    'No original startup, HTML builder, storage, network or application is executed.',
    'Numeric source JSON comparisons are checked within safe JavaScript integer precision. The rewrite retains exact signed i64 identities.',
    'Canonical page thread IDs are qualified; historical URL aliases and general numeric coercion remain outside this reference.'],
  excerpts,
};
if (args[0] === '--write') await writeFile(target, `${JSON.stringify(fixture, null, 2)}\n`);
else assert.deepEqual(JSON.parse(await readFile(target, 'utf8')), fixture, 'Quote spelling source extraction drift');
console.log('Five isolated source quote functions match the pinned original extension.');
