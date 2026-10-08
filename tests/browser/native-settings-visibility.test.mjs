import assert from 'node:assert/strict';
import { test } from 'node:test';
import { settingAvailable } from '../../apps/public/static/native-settings.v1.js';

// Independent source matrix: SettingsMenu.options, extension.js8966–9138 at
// 545b781, restricted to implemented controls. Import requires no DOM setup.
const groups = [
  ['quotePreview backlinks inlineQuotes quickReply persistentQR', 'quotePreview backlinks quickReply'],
  ['threadUpdater alwaysAutoUpdate threadWatcher threadAutoWatcher autoScroll updaterSound fixedThreadWatcher threadStats', 'threadUpdater alwaysAutoUpdate threadWatcher threadAutoWatcher threadStats'],
  ['filter threadHiding hideStubs', 'threadHiding'],
  ['threadExpansion dropDownNav classicNav autoHideNav customMenu alwaysDepage topPageNav stickyNav keyBinds', 'threadExpansion alwaysDepage'],
  ['imageExpansion fitToScreenExpansion imageHover imageHoverBg revealSpoilers noPictures embedYouTube embedSoundCloud', 'imageExpansion revealSpoilers noPictures'],
  ['linkify darkTheme customCSS IDColor compactThreads centeredThreads localTime', 'linkify darkTheme customCSS IDColor localTime'],
].map(([all, mobile]) => ({ all: all.split(' '), mobile: mobile.split(' ') }));

test('mobile availability matches every source category exactly', () => {
  for (const group of groups) {
    assert.deepEqual(group.all.filter(key => settingAvailable(key, true)), group.mobile);
  }
});

test('desktop retains all supported settings except mobile-only darkTheme', () => {
  for (const group of groups) {
    assert.deepEqual(group.all.filter(key => settingAvailable(key, false)), group.all.filter(key => key !== 'darkTheme'));
  }
});

test('global disableAll is available on both layouts; unsupported keys stay unavailable', () => {
  for (const mobile of [false, true]) {
    assert.equal(settingAvailable('disableAll', mobile), true);
    for (const key of ['unmuteWebm', 'forceHTTPS', 'unknownFutureSetting', '__proto__', 'constructor', '', null, undefined]) {
      assert.equal(settingAvailable(key, mobile), false, String(key));
    }
  }
});
