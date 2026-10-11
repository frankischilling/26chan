import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const target = new URL('../fixtures/source-linkification.json', import.meta.url);
const sourceHash = '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31';
const excerptHash = '16e8c25d177f3651e90b8ee548656129f23ff17beba92064db490a2d612cc124';
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const args = process.argv.slice(2);
assert.ok(['--write', '--check'].includes(args[0]) &&
  (args.length === 1 || (args.length === 3 && args[1] === '--source')),
  'Use --write or --check, optionally followed by --source ORIGINAL_DIRECTORY');
const sourceFile = args.length === 3 ? path.join(args[2], 'js', 'extension.js')
  : new URL('../4chan-old/js/extension.js', import.meta.url);
let raw;
try { raw = await readFile(sourceFile); }
catch (error) {
  if (args[0] !== '--check' || args.length !== 1 || error.code !== 'ENOENT') throw error;
  const fixture = JSON.parse(await readFile(target, 'utf8'));
  assert.equal(fixture.source_sha256, sourceHash);
  assert.equal(fixture.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
  assert.equal(fixture.excerpt_sha256, excerptHash);
  assert.equal(sha(fixture.excerpt), excerptHash);
  assert.deepEqual(fixture.defaults, { desktop: false, mobile: true });
  console.log('Captured Linkify object matches the pinned source hash; original tree is absent.');
  process.exit(0);
}
assert.equal(sha(raw), sourceHash, 'Original extension source drift');
const source = raw.toString('utf8');
const start = source.indexOf('var Linkify = {');
assert.ok(start >= 0 && source.indexOf('var Linkify = {', start + 1) === -1);
const end = source.indexOf('\n};', start) + 4;
assert.ok(end > start);
const excerpt = source.slice(start, end);
assert.equal(sha(excerpt), excerptHash, 'Original Linkify object drift');
const defaultValue = name => {
  const start = source.indexOf(`var ${name} = {`);
  assert.ok(start >= 0);
  const body = source.slice(start, source.indexOf('\n};', start));
  const matches = [...body.matchAll(/\blinkify: (true|false),?/g)];
  assert.equal(matches.length, 1);
  return matches[0][1] === 'true';
};
const fixture = {
  source_revision: '545b7812d1849f7958d914950c91fdbbe38f6b22',
  source_file: 'js/extension.js', source_sha256: sourceHash,
  excerpt_sha256: excerptHash,
  defaults: { desktop: defaultValue('Config'), mobile: defaultValue('ConfigMobile') },
  limits: [
    'Only this extracted object executes on bounded synthetic strings in an isolated context.',
    'No original application startup, configuration loading, storage or network executes.',
    'Its serialized HTML output is reference evidence, never application rendering input.',
  ],
  excerpt,
};
if (args[0] === '--write') await writeFile(target, `${JSON.stringify(fixture, null, 2)}\n`);
else assert.deepEqual(JSON.parse(await readFile(target, 'utf8')), fixture, 'Linkify extraction drift');
console.log('Isolated Linkify object and desktop/mobile defaults match the pinned original extension.');
