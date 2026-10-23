import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
const source = process.argv[2];
assert.ok(source && process.argv.length === 3, 'Pass the supplied reference directory');
const fixture = JSON.parse(await readFile('fixtures/seasonal-event-reference.json', 'utf8'));
const pins = {
  'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
  'js/core.js': 'a9ab67bea1f51fcdaaac1a879098a4d5888622008bf7f26f1fe18861ea52aab5',
};
assert.deepEqual(fixture.sources, pins);
const files = {};
for (const [name, hash] of Object.entries(pins)) {
  const bytes = await readFile(path.join(source, name));
  assert.equal(createHash('sha256').update(bytes).digest('hex'), hash);
  files[name] = bytes.toString('utf8').replaceAll('\r\n', '\n');
}
function extract(source_, name, indent = '') {
  const start = source_.indexOf('function ' + name + '(');
  const end = source_.indexOf('\n' + indent + '}', start);
  assert.ok(start >= 0 && end > start);
  return source_.slice(start, end + indent.length + 2);
}
assert.deepEqual(fixture.functions, {
  initial: extract(files['js/core.js'], 'initStyleSheet'),
  change: extract(files['js/core.js'], 'setActiveStyleSheet'),
  rotate: extract(files['imgboard.php'], 'fc_skelrot', '  '),
  spooky_init: extract(files['imgboard.php'], 'fc_spooky_init', '  '),
  spooky_cleanup: extract(files['imgboard.php'], 'fc_spooky_cleanup', '  '),
  snow_init: extract(files['imgboard.php'], 'fc_tomorrow_init', '  '),
});
console.log('Seasonal function slices match the supplied source.');
