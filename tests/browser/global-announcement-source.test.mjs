import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { buildReference } from '../../scripts/record-global-announcement.mjs';
import { announcementDismissed } from '../../apps/public/static/page-chrome.v1.js';
const reference = JSON.parse(await readFile(new URL('../fixtures/global-announcement-source.json', import.meta.url)));
const snippets = JSON.parse(await readFile(new URL('../fixtures/global-announcement-snippets.json', import.meta.url)));

test('announcement vectors come from the pinned original functions', () => {
  assert.deepEqual(buildReference(snippets), reference);
  for (const key of Object.keys(snippets)) {
    assert.throws(() => buildReference({ ...snippets, [key]: { ...snippets[key], snippet: snippets[key].snippet + ' ' } }));
  }
});
test('all source timestamp comparisons retain string ordering and missing-value semantics', () => {
  for (const row of reference.cases) {
    assert.equal(announcementDismissed(row.current, row.previous), row.states[0].hidden,
      `${row.mode}/${row.current}/${row.previous}`);
  }
  for (const current of ['', 'one', '1'.repeat(21), null, {}, 1]) assert.equal(announcementDismissed(current, '2'), false);
  assert.equal(announcementDismissed('1', '2'.repeat(65)), false);
});
