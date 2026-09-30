import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { mobileHeaderLabel } from '../../apps/public/client/native-post-numbers.js';

const reference = JSON.parse(await readFile(new URL('../../docs/public-mobile-label-reference.json', import.meta.url), 'utf8'));
const release = JSON.parse(await readFile(new URL('../../docs/public-watcher-assets.json', import.meta.url), 'utf8'));

test('mobile labels match pinned pure-helper vectors, including serialized length and UTF-16 boundaries', () => {
  assert.deepEqual(reference.client, { url: release.source, sha256: release.source_sha256 });
  assert.equal(reference.cases.length, 10);
  for (const row of reference.cases) {
    assert.deepEqual(mobileHeaderLabel(row.input), { text: row.visible, shortened: row.shortened }, row.name);
  }
  const split = reference.cases.find(row => row.name === 'split surrogate boundary');
  assert.equal(split.sourceUnits[29], 0xd83d);
  assert.equal(split.visible[29], '\ufffd');
});
