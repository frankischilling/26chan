import assert from 'node:assert/strict';
import { test } from 'node:test';
import { captureSettingsPresentation, settingsOptionChecked, settingsStartupDefaults, readSettingsStartup } from '../../apps/public/static/native-settings.v1.js';

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

// Independently enumerated from source Config (8795–8842), ConfigMobile
// (8844–8848), and the first-run mobile-device override (9483–9486).
const sourceDefaults = {
  quotePreview: true, backlinks: true, quickReply: true, threadUpdater: true, threadHiding: true,
  alwaysAutoUpdate: false, topPageNav: false, threadWatcher: false, threadAutoWatcher: false,
  imageExpansion: true, fitToScreenExpansion: false, threadExpansion: true, alwaysDepage: false,
  localTime: true, stickyNav: false, keyBinds: false, inlineQuotes: false, filter: false,
  revealSpoilers: false, imageHover: false, threadStats: true, IDColor: true, noPictures: false,
  embedYouTube: true, embedSoundCloud: false, updaterSound: false, customCSS: false,
  autoScroll: false, hideStubs: false, compactThreads: false, centeredThreads: false,
  dropDownNav: false, autoHideNav: false, classicNav: false, fixedThreadWatcher: false,
  persistentQR: false, forceHTTPS: false, darkTheme: false, linkify: false, unmuteWebm: false,
  disableAll: false,
};

for (const mobileLayout of [false, true]) {
  for (const mobileDevice of [false, true]) {
    for (const disabled of [false, true]) {
      test(`source startup defaults: layout=${mobileLayout}, device=${mobileDevice}, disabled=${disabled}`, () => {
        const expected = { ...sourceDefaults,
          ...(!disabled && mobileLayout ? { embedYouTube: false, compactThreads: false, linkify: true } : {}),
          ...(!disabled && mobileDevice ? { topPageNav: false, dropDownNav: true } : {}),
        };
        const actual = settingsStartupDefaults({ mobileLayout, mobileDevice, disabled });
        assert.deepEqual(actual, expected);
        assert.equal(Object.keys(actual).length, Object.keys(sourceDefaults).length);
        assert.ok(Object.values(actual).every(value => typeof value === 'boolean'));
        assert.equal(Object.hasOwn(actual, 'firstRun'), false);
        assert.equal(Object.hasOwn(actual, 'mobileLayout'), false);
      });
    }
  }
}

test('startup defaults return independent snapshots and disabled defaults omit mobile overrides', () => {
  const options = { mobileLayout: true, mobileDevice: true };
  const first = settingsStartupDefaults(options);
  const second = settingsStartupDefaults(options);
  assert.notEqual(first, second);
  assert.deepEqual(first, second);
  options.mobileLayout = false;
  assert.equal(first.linkify, true);
  assert.deepEqual(settingsStartupDefaults({ mobileLayout: true, mobileDevice: true, disabled: true }), sourceDefaults);
  assert.deepEqual(settingsStartupDefaults({ mobileLayout: false, mobileDevice: false }), sourceDefaults);
});

test('startup reader distinguishes absence from malformed and bounded non-record values', () => {
  for (const raw of [null, '', '{}']) assert.deepEqual(readSettingsStartup(raw), { status: 'ok', settings: {} });
  for (const raw of [undefined, false, 0, '{', ' ', 'null', 'false', '1', '"text"', '[]', '[{}]', 'x'.repeat(4097),
    JSON.stringify({ future: 'x'.repeat(4097) }), '{"future":1e400}', '{"future":[-1e400]}']) {
    assert.deepEqual(readSettingsStartup(raw), { status: 'invalid' }, String(raw).slice(0, 80));
  }
});

test('startup reader validates every source boolean without losing safe unknown settings', () => {
  for (const key of [...Object.keys(sourceDefaults), 'customMenu', 'imageHoverBg']) {
    for (const value of [false, true]) {
      assert.deepEqual(readSettingsStartup(JSON.stringify({ [key]: value })), { status: 'ok', settings: { [key]: value } });
    }
    for (const value of [null, 0, 1, 'false', [], {}]) {
      assert.deepEqual(readSettingsStartup(JSON.stringify({ [key]: value })), { status: 'invalid' }, key);
    }
  }
  const settings = { quotePreview: false, disableAll: true, customMenu: false, customMenuList: 'a-b',
    futureOption: { nested: ['allowed', 2, true, null] }, topPageNavPosition: { left: 30, top: 50 } };
  const raw = JSON.stringify(settings);
  assert.deepEqual(readSettingsStartup(raw), { status: 'ok', settings });
  const first = readSettingsStartup(raw);
  first.settings.futureOption.nested.push('changed');
  assert.deepEqual(readSettingsStartup(raw), { status: 'ok', settings });
});

test('startup reader rejects reserved keys even below arrays and unknown objects', () => {
  for (const key of ['__proto__', 'prototype', 'constructor']) {
    for (const raw of [`{"${key}":{}}`, `{"future":{"${key}":false}}`, `{"future":[{"${key}":{}}]}`]) {
      assert.deepEqual(readSettingsStartup(raw), { status: 'invalid' }, raw);
    }
  }
});
