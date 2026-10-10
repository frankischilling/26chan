import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sourceHash = '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31';
const target = new URL('../tests/fixtures/native-https-preference-source.json', import.meta.url);
const args = process.argv.slice(2);
const check = args[0] === '--check';
if (check) args.shift();
let reference;
if (args.length) {
  assert.equal(args.shift(), '--source');
  const path = args.shift();
  assert.ok(path); assert.equal(args.length, 0);
  const bytes = await readFile(path);
  assert.equal(hash(bytes), sourceHash, 'Unexpected supplied extension.js');
  const source = bytes.toString('utf8').replaceAll('\r\n', '\n');
  const fn = name => {
    const start = source.indexOf(`${name} = function(`);
    assert.ok(start >= 0, name);
    const end = source.indexOf('\n};', start);
    assert.ok(end > start);
    return source.slice(start, end + 3);
  };
  const init = fn('Main.init');
  const end = init.indexOf('  if (Main.firstRun && Config.loadFromURL())');
  assert.ok(end > 0);
  const setting = source.split('\n').find(line => line.trimStart().startsWith('forceHTTPS: [ '));
  assert.ok(setting);
  reference = {
    source: 'js/extension.js', revision: '545b7812d1849f7958d914950c91fdbbe38f6b22', sha256: sourceHash,
    snippets: Object.fromEntries(Object.entries({
      load: fn('Config.load'), save: fn('Config.save'), init: init.slice(0, end) + '};', setting,
    }).map(([name, text]) => [name, { sha256: hash(text), text }])),
  };
  if (check) assert.deepEqual(reference, JSON.parse(await readFile(target)));
  else await writeFile(target, JSON.stringify(reference, null, 2) + '\n');
} else {
  assert.ok(check, 'Pass --source with the supplied extension.js to record');
  reference = JSON.parse(await readFile(target));
}
assert.equal(reference.sha256, sourceHash);
assert.deepEqual(Object.keys(reference.snippets), ['load', 'save', 'init', 'setting']);
for (const row of Object.values(reference.snippets)) assert.equal(hash(row.text), row.sha256);
assert.match(reference.snippets.setting.text, /Always use HTTPS.*true/);
console.log('Pinned HTTPS preference source snippets verified');
