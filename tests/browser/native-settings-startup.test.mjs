import assert from 'node:assert/strict';
import { test } from 'node:test';
import { captureSettingsPresentation, settingsOptionChecked } from '../../apps/public/static/native-settings.v1.js';

// Source Config.load/SettingsMenu/Main.init: raw falsiness determines firstRun,
// and layout is captured at startup. These are presentation flags only.
test('only a successful absent or empty raw read selects first-run presentation', () => {
  for (const raw of [null, '']) {
    assert.equal(captureSettingsPresentation({ status: 'ok', raw }, false).firstRun, true);
  }
  for (const raw of ['{}', '{', 'null', 'false', '[]', ' ', 'x'.repeat(4097), undefined, false, 0]) {
    assert.equal(captureSettingsPresentation({ status: 'ok', raw }, false).firstRun, false);
  }
  for (const result of [undefined, null, { status: 'unavailable' }, { status: 'unavailable', raw: null }, { raw: null }]) {
    assert.equal(captureSettingsPresentation(result, false).firstRun, false);
  }
});

test('presentation is an immutable snapshot and cannot authorize initialization', () => {
  const read = { status: 'ok', raw: '' };
  const startup = captureSettingsPresentation(read, true);
  read.raw = '{}';
  read.status = 'unavailable';
  assert.deepEqual(startup, { firstRun: true, mobileLayout: true });
  assert.equal(Object.isFrozen(startup), true);
  assert.throws(() => { startup.mobileLayout = false; }, TypeError);
  assert.deepEqual(Object.keys(startup), ['firstRun', 'mobileLayout']);
  assert.deepEqual(captureSettingsPresentation({ status: 'ok', raw: '{}' }, false),
    { firstRun: false, mobileLayout: false });
});

test('layout snapshot is strict and independent of storage availability', () => {
  for (const value of [false, undefined, null, 1, 'true']) {
    assert.equal(captureSettingsPresentation({ status: 'ok', raw: null }, value).mobileLayout, false);
  }
  assert.equal(captureSettingsPresentation({ status: 'unavailable' }, true).mobileLayout, true);
});

test('checkbox layout overrides use the snapshot while honoring fresh saved values', () => {
  for (const mobileLayout of [false, true]) {
    const startup = captureSettingsPresentation({ status: 'ok', raw: '{}' }, mobileLayout);
    assert.equal(settingsOptionChecked('linkify', {}, startup), mobileLayout);
    assert.equal(settingsOptionChecked('embedYouTube', {}, startup), !mobileLayout);
    for (const value of [false, true]) {
      const settings = Object.freeze({ linkify: value, embedYouTube: value });
      assert.equal(settingsOptionChecked('linkify', settings, startup), mobileLayout || value);
      assert.equal(settingsOptionChecked('embedYouTube', settings, startup), value);
      assert.equal(settingsOptionChecked('linkify', { ...settings, disableAll: true }, startup), value);
    }
    assert.equal(settingsOptionChecked('quotePreview', {}, startup), undefined);
  }
});
