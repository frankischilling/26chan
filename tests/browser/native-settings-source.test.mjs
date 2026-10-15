import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { buildReference, parseLiteral } from '../../scripts/record-native-settings-options.mjs';
import { settingAvailable, settingsStartupDefaults } from '../../apps/public/static/native-settings.v1.js';

const reference = JSON.parse(await readFile(new URL('../fixtures/native-settings-options-source.json', import.meta.url)));
const snippets = JSON.parse(await readFile(new URL('../fixtures/native-settings-options-snippets.json', import.meta.url)));

test('complete Settings fixture is derived from pinned literal source data', () => {
  assert.deepEqual(JSON.parse(JSON.stringify(buildReference(snippets))), reference);
  for (const key of Object.keys(snippets)) {
    assert.throws(() => buildReference({ ...snippets, [key]: snippets[key] + ' ' }));
  }
  for (const input of ['{value: run()}', '{value: true, value: false}', '{value: null}', '[true];', '{value: "unterminated}']) {
    assert.throws(() => parseLiteral(input));
  }
});

for (const mobileLayout of [false, true]) for (const mobileDevice of [false, true]) for (const disabled of [false, true]) {
  test(`complete source defaults: layout=${mobileLayout}, device=${mobileDevice}, disabled=${disabled}`, () => {
    const expected = { ...reference.defaults };
    if (!disabled && mobileLayout) Object.assign(expected, reference.mobile);
    if (!disabled && mobileDevice) Object.assign(expected, { topPageNav: false, dropDownNav: true });
    assert.deepEqual(settingsStartupDefaults({ mobileLayout, mobileDevice, disabled }), expected);
  });
}

test('every source Settings option has the correct desktop and mobile availability', () => {
  const seen = new Set();
  for (const group of reference.groups) for (const option of group.options) {
    assert.equal(seen.has(option.key), false); seen.add(option.key);
    for (const mobile of [false, true]) for (const httpsAvailable of [false, true]) {
      const expected = option.key === 'forceHTTPS' ? httpsAvailable
        : mobile ? option.mobile : !option.mobileOnly;
      assert.equal(settingAvailable(option.key, mobile, httpsAvailable), expected,
        `${group.name}/${option.key}: mobile=${mobile}, HTTPS=${httpsAvailable}`);
    }
  }
  assert.ok(seen.has('unmuteWebm'));
  assert.equal(reference.groups.length, 6);
});
