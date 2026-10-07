import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from '@playwright/test';

// Category order and layout availability follow 4chan-old/js/extension.js at
// 545b781, restricted to the rewrite's implemented controls.
const groups = [
  ['Quotes & Replying', 'quotePreview backlinks inlineQuotes quickReply persistentQR'],
  ['Monitoring', 'threadUpdater alwaysAutoUpdate threadWatcher threadAutoWatcher autoScroll updaterSound fixedThreadWatcher threadStats'],
  ['Filters & Post Hiding', 'filter threadHiding hideStubs'],
  ['Navigation', 'threadExpansion dropDownNav classicNav autoHideNav customMenu alwaysDepage topPageNav stickyNav keyBinds'],
  ['Images & Media', 'imageExpansion fitToScreenExpansion imageHover imageHoverBg revealSpoilers noPictures embedYouTube embedSoundCloud'],
  ['Miscellaneous', 'linkify darkTheme customCSS IDColor compactThreads centeredThreads localTime'],
].map(([name, keys]) => ({ name, keys: keys.split(' ') }));
const mobileKeys = new Set('quotePreview backlinks quickReply threadUpdater alwaysAutoUpdate threadWatcher threadAutoWatcher threadStats threadHiding threadExpansion alwaysDepage imageExpansion revealSpoilers noPictures linkify darkTheme customCSS IDColor localTime'.split(' '));
const expectedGroups = mobile => groups.map(group => ({ ...group,
  keys: group.keys.filter(key => mobile ? mobileKeys.has(key) : key !== 'darkTheme'),
}));
// These are isolated installSettings fallback defaults, not integrated app defaults.
// The app supplies optionChecked (including embedYouTube's enabled default).
const enabledByDefault = new Set('threadHiding threadUpdater threadExpansion threadStats quickReply quotePreview backlinks imageExpansion localTime IDColor'.split(' '));
const sources = new Map(await Promise.all(['native-settings.v1.js', 'native-custom-css.v1.js'].map(async name =>
  [`/static/${name}`, await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8')])));

async function fixture(page, { settings, mobile = false, override = {},
  raw = settings === undefined ? null : JSON.stringify(settings), unavailable = false, integratedDefaults = false } = {}) {
  settings ??= {};
  await page.setViewportSize({ width: mobile ? 390 : 1000, height: 800 });
  const context = page.context();
  await context.route('**/*', route => {
    const path = new URL(route.request().url()).pathname;
    if (sources.has(path)) return route.fulfill({ contentType: 'text/javascript', body: sources.get(path) });
    if (route.request().isNavigationRequest()) return route.fulfill({ contentType: 'text/html', body:
      '<!doctype html><html><body><div class="boardList"></div></body></html>' });
    return route.abort();
  });
  await page.goto('https://settings.example/demo/');
  await page.evaluate(async ({ settings, override, raw, unavailable, integratedDefaults }) => {
    if (raw === null) localStorage.removeItem('4chan-settings');
    else localStorage.setItem('4chan-settings', raw);
    window.storageWrites = [];
    for (const method of ['setItem', 'removeItem', 'clear']) {
      const original = Storage.prototype[method];
      Storage.prototype[method] = function (...args) {
        storageWrites.push([method, ...args]);
        return original.apply(this, args);
      };
    }
    window.settingsState = settings;
    window.saveCalls = [];
    window.callbacks = [];
    window.saveMode = 'success';
    window.savedEvents = 0;
    document.addEventListener('4chanSettingsSaved', () => savedEvents++);
    const callback = name => source => callbacks.push([name, source?.id]);
    const { installSettings, captureSettingsPresentation, settingsOptionChecked } = await import('/static/native-settings.v1.js');
    const presentation = captureSettingsPresentation(unavailable ? { status: 'unavailable' }
      : { status: 'ok', raw: localStorage.getItem('4chan-settings') }, matchMedia('(max-width: 480px)').matches);
    window.settingsAPI = installSettings({
      catalog: false,
      read: () => ({ ...settingsState }),
      presentation,
      optionChecked: (key, initial, startup) => override[key]
        ?? (integratedDefaults ? settingsOptionChecked(key, initial, startup) : undefined),
      toggleWatcher: callback('watcher'), openFilters: callback('filters'),
      clearThreads: callback('clear'), openKeybinds: callback('keys'),
      openCustomMenu: callback('menu'), openCustomCSS: callback('css'), openExport: callback('export'),
      save: async (changes, signal) => {
        const call = { changes, aborted: false };
        saveCalls.push(call);
        signal.addEventListener('abort', () => { call.aborted = true; }, { once: true });
        if (saveMode === 'failure') return false;
        if (saveMode === 'throw') throw new Error('fixture failure');
        if (saveMode === 'delayed') await new Promise(resolve => { window.releaseSave = resolve; });
        if (signal.aborted) return false;
        settingsState = { ...settingsState, ...changes };
        return { persisted: false };
      },
    });
  }, { settings, override, raw, unavailable, integratedDefaults });
  await page.locator('#settingsWindowLink').click();
  return page;
}

async function categoryState(page) {
  return page.locator('.settings-cat-lbl button').evaluateAll(buttons => buttons.map(button => {
    const list = document.getElementById(button.getAttribute('aria-controls'));
    return { name: button.textContent, expanded: button.getAttribute('aria-expanded'), hidden: list.hidden,
      keys: [...list.querySelectorAll('[data-option]')].map(input => input.dataset.option) };
  }));
}

for (const mobile of [false, true]) {
  test(`${mobile ? 'mobile' : 'desktop'} has exactly six ordered groups, source-visible controls and fallback defaults`, async ({ page }) => {
    await fixture(page, { mobile });
    const expected = expectedGroups(mobile);
    assert.deepEqual((await categoryState(page)).map(({ name, keys }) => ({ name, keys })), expected);
    const controls = await page.locator('[data-option]').evaluateAll(inputs => inputs.map(input => ({
      key: input.dataset.option, id: input.id, checked: input.checked, label: input.parentElement.textContent.trim(),
    })));
    assert.deepEqual(controls.map(input => input.key), [...expected.flatMap(group => group.keys), 'disableAll']);
    assert.equal(new Set(controls.map(input => input.key)).size, controls.length);
    for (const input of controls) {
      assert.equal(input.id, `setting-${input.key}`);
      assert.equal(input.checked, enabledByDefault.has(input.key), input.key);
      assert.ok(input.label.length > 0);
    }
    assert.equal(await page.locator('.settings-cat #setting-disableAll').count(), 0);
    assert.equal(await page.locator('#setting-darkTheme').count(), mobile ? 1 : 0);
    assert.equal(await page.locator('#setting-unmuteWebm, #setting-forceHTTPS').count(), 0);
    assert.deepEqual(await page.locator('.settings-sub input').evaluateAll(inputs => inputs.map(input => input.dataset.option)),
      mobile ? ['threadAutoWatcher'] : ['threadAutoWatcher', 'classicNav', 'autoHideNav', 'imageHoverBg']);
    assert.equal(await page.locator('#settings-export').textContent(), 'Export Settings');
    assert.equal(await page.locator('#settings-save').textContent(), 'Save Settings');
    assert.equal(await page.evaluate(() => document.activeElement.id), 'setting-quotePreview');
    assert.deepEqual(await page.evaluate(() => storageWrites), []);
  });
}

test('independent disclosure and Expand All do not persist; reopening retains startup disclosure', async ({ page }) => {
  await fixture(page, { settings: { darkTheme: true } });
  assert.ok((await categoryState(page)).every(group => group.hidden && group.expanded === 'false'));
  assert.equal(await page.evaluate(() => document.activeElement.getAttribute('aria-label')), 'Quotes & Replying');
  for (const { name } of groups) {
    const button = page.getByRole('button', { name, exact: true });
    await button.click();
    assert.deepEqual((await categoryState(page)).filter(group => !group.hidden).map(group => group.name), [name]);
    assert.equal(await button.getAttribute('aria-expanded'), 'true');
    await button.click();
    assert.ok((await categoryState(page)).every(group => group.hidden));
  }
  await page.locator('#settings-expand-all').click();
  await page.locator('#settings-expand-all').click();
  assert.ok((await categoryState(page)).every(group => !group.hidden && group.expanded === 'true'));
  await page.locator('#settings-close').click();
  assert.equal(await page.evaluate(() => document.activeElement.id), 'settingsWindowLink');
  await page.locator('#settingsWindowLink').click();
  assert.ok((await categoryState(page)).every(group => group.hidden));
  await page.keyboard.press('Escape');
  await page.evaluate(() => { settingsState = {}; localStorage.removeItem('4chan-settings'); storageWrites.length = 0; });
  await page.locator('#settingsWindowLink').click();
  assert.ok((await categoryState(page)).every(group => group.hidden));
  assert.deepEqual(await page.evaluate(() => ({ writes: storageWrites, saves: saveCalls })), { writes: [], saves: [] });
});

test('cancel discards edits across all groups and resizing does not replace the open form', async ({ page }) => {
  await fixture(page);
  for (const group of groups) {
    const control = page.locator(`#setting-${group.keys[0]}`);
    await control.setChecked(!(await control.isChecked()));
  }
  await page.locator('#setting-inlineQuotes').check();
  await page.setViewportSize({ width: 390, height: 800 });
  assert.equal(await page.locator('#setting-inlineQuotes').count(), 1);
  assert.equal(await page.locator('#setting-inlineQuotes').isChecked(), true);
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('#settingsMenu').count(), 0);
  assert.equal(await page.evaluate(() => document.activeElement.id), 'settingsWindowLink');
  await page.locator('#settingsWindowLink').click();
  assert.deepEqual((await categoryState(page)).map(({ name, keys }) => ({ name, keys })), expectedGroups(false));
  assert.equal(await page.locator('#setting-inlineQuotes').isChecked(), false);
  assert.equal(await page.locator('#setting-darkTheme').count(), 0);
  for (const group of expectedGroups(false)) assert.equal(await page.locator(`#setting-${group.keys[0]}`).isChecked(), enabledByDefault.has(group.keys[0]));
  assert.ok((await categoryState(page)).every(group => !group.hidden));
  assert.deepEqual(await page.evaluate(() => ({ writes: storageWrites, saves: saveCalls, settings: settingsState })),
    { writes: [], saves: [], settings: {} });
});

test('callbacks retain their openers and save submits only changed controls while merging fresh state', async ({ page }) => {
  await fixture(page, { override: { embedYouTube: true, backlinks: false } });
  assert.equal(await page.locator('#setting-embedYouTube').isChecked(), true);
  assert.equal(await page.locator('#setting-backlinks').isChecked(), false);
  for (const id of ['filters-edit', 'thread-hiding-clear', 'custom-menu-edit', 'keybinds-open', 'custom-css-edit', 'settings-export']) {
    await page.locator(`#${id}`).click();
  }
  assert.deepEqual(await page.evaluate(() => callbacks), [['filters', 'filters-edit'], ['clear', undefined],
    ['menu', 'custom-menu-edit'], ['keys', 'keybinds-open'], ['css', 'custom-css-edit'], ['export', 'settings-export']]);
  await page.locator('#setting-quotePreview').uncheck();
  await page.locator('#setting-linkify').check();
  await page.evaluate(() => { settingsState = { threadWatcher: true, unknownFutureSetting: 'preserved' }; });
  await page.locator('#settings-save').click();
  await page.waitForFunction(() => !document.getElementById('settingsMenu'));
  assert.deepEqual(await page.evaluate(() => saveCalls[0].changes), { quotePreview: false, linkify: true });
  assert.deepEqual(await page.evaluate(() => settingsState), {
    threadWatcher: true, unknownFutureSetting: 'preserved', quotePreview: false, linkify: true,
  });
  assert.equal(await page.evaluate(() => savedEvents), 1);
  assert.deepEqual(await page.evaluate(() => storageWrites), []);
});

test('save errors remain recoverable and closing aborts pending saves without applying stale changes', async ({ page }) => {
  await fixture(page);
  await page.locator('#setting-linkify').check();
  for (const mode of ['failure', 'throw']) {
    await page.evaluate(value => { saveMode = value; }, mode);
    await page.locator('#settings-save').click();
    await page.waitForFunction(() => document.querySelector('.settingsMessage').textContent.includes('could not be saved'));
    assert.equal(await page.locator('#settings-save').isEnabled(), true);
    assert.equal(await page.locator('#settings-export').isEnabled(), true);
  }
  await page.evaluate(() => { saveMode = 'delayed'; });
  await page.locator('#settings-save').click();
  assert.equal(await page.locator('#settings-export').isDisabled(), true);
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => saveCalls.at(-1).aborted), true);
  await page.locator('#settingsWindowLink').click();
  await page.evaluate(async () => { releaseSave(); await new Promise(resolve => setTimeout(resolve, 0)); });
  assert.equal(await page.locator('#settingsMenu').count(), 1);
  assert.equal(await page.locator('#setting-linkify').isChecked(), false);
  assert.deepEqual(await page.evaluate(() => ({ settings: settingsState, events: savedEvents, writes: storageWrites })),
    { settings: {}, events: 0, writes: [] });
});

for (const mobile of [false, true]) {
  test(`${mobile ? 'mobile' : 'desktop'} save preserves hidden saved preferences and concurrent unseen changes`, async ({ page }) => {
    const hiddenKeys = groups.flatMap(group => group.keys)
      .filter(key => mobile ? !mobileKeys.has(key) : key === 'darkTheme');
    const hidden = Object.fromEntries(hiddenKeys.map((key, index) => [key, index % 2 === 0]));
    await fixture(page, { mobile, settings: { ...hidden, linkify: false } });
    await page.locator('#settings-expand-all').click();
    for (const key of hiddenKeys) assert.equal(await page.locator(`#setting-${key}`).count(), 0, key);
    if (mobile) {
      assert.equal(await page.locator('#filters-edit, #custom-menu-edit, #keybinds-open').count(), 0);
      await page.locator('#thread-hiding-clear').click();
      await page.locator('#custom-css-edit').click();
      assert.deepEqual(await page.evaluate(() => callbacks), [['clear', undefined], ['css', 'custom-css-edit']]);
    }
    await page.locator('#setting-linkify').check();
    const concurrent = { [hiddenKeys[0]]: !hidden[hiddenKeys[0]], unknownFutureSetting: 'preserved' };
    await page.evaluate(changes => { Object.assign(settingsState, changes); }, concurrent);
    await page.locator('#settings-save').click();
    await page.waitForFunction(() => !document.getElementById('settingsMenu'));
    assert.deepEqual(await page.evaluate(() => saveCalls[0].changes), { linkify: true });
    assert.deepEqual(await page.evaluate(() => settingsState), { ...hidden, ...concurrent, linkify: true });
    assert.deepEqual(await page.evaluate(() => storageWrites), []);
    await page.locator('#settingsWindowLink').click();
    assert.deepEqual((await categoryState(page)).map(({ name, keys }) => ({ name, keys })), expectedGroups(mobile));
    await page.locator('#settings-expand-all').click();
    assert.equal(await page.locator('#setting-linkify').isChecked(), true);
    await page.setViewportSize({ width: mobile ? 1000 : 390, height: 800 });
    // Availability is captured at startup; resizing must leave this draft intact.
    assert.deepEqual((await categoryState(page)).map(({ name, keys }) => ({ name, keys })), expectedGroups(mobile));
    await page.keyboard.press('Escape');
    await page.locator('#settingsWindowLink').click();
    assert.deepEqual((await categoryState(page)).map(({ name, keys }) => ({ name, keys })), expectedGroups(mobile));
    for (const key of hiddenKeys) assert.equal(await page.locator(`#setting-${key}`).count(), 0, key);
    // A new document captures the new layout, with saved hidden values intact.
    await fixture(page, { mobile: !mobile, settings: { ...hidden, ...concurrent, linkify: true } });
    assert.deepEqual((await categoryState(page)).map(({ name, keys }) => ({ name, keys })), expectedGroups(!mobile));
    await page.locator('#settings-expand-all').click();
    for (const key of hiddenKeys) {
      assert.equal(await page.locator(`#setting-${key}`).isChecked(), { ...hidden, ...concurrent }[key], key);
    }
  });
}

for (const [name, raw, firstRun, unavailable] of [
  ['absent', null, true, false], ['empty string', '', true, false],
  ['stored empty object', '{}', false, false], ['malformed', '{', false, false],
  ['unavailable', null, false, true],
]) {
  test(`${name} raw storage captures first-run disclosure without writes`, async ({ page }) => {
    await fixture(page, { raw, unavailable });
    assert.ok((await categoryState(page)).every(group => group.hidden === !firstRun));
    await page.keyboard.press('Escape');
    await page.evaluate(() => { settingsState = { quotePreview: false }; });
    await page.locator('#settingsWindowLink').click();
    assert.ok((await categoryState(page)).every(group => group.hidden === !firstRun));
    await page.locator('#settings-expand-all').click();
    assert.equal(await page.locator('#setting-quotePreview').isChecked(), false);
    assert.deepEqual(await page.evaluate(() => ({ writes: storageWrites, saves: saveCalls })), { writes: [], saves: [] });
  });
}

for (const mobile of [false, true]) {
  test(`${mobile ? 'mobile' : 'desktop'} checkbox overrides retain startup layout with fresh values`, async ({ page }) => {
    await fixture(page, { mobile, integratedDefaults: true });
    assert.equal(await page.locator('#setting-linkify').isChecked(), mobile);
    if (!mobile) assert.equal(await page.locator('#setting-embedYouTube').isChecked(), true);
    await page.locator('#setting-linkify').setChecked(!mobile);
    await page.setViewportSize({ width: mobile ? 1000 : 390, height: 800 });
    assert.equal(await page.locator('#setting-linkify').isChecked(), !mobile);
    await page.keyboard.press('Escape');
    await page.locator('#settingsWindowLink').click();
    assert.equal(await page.locator('#setting-linkify').isChecked(), mobile);
    if (!mobile) assert.equal(await page.locator('#setting-embedYouTube').isChecked(), true);
    await page.keyboard.press('Escape');
    await page.evaluate(() => { settingsState = { disableAll: true, linkify: false, embedYouTube: false }; });
    await page.locator('#settingsWindowLink').click();
    assert.equal(await page.locator('#setting-linkify').isChecked(), false);
    if (!mobile) assert.equal(await page.locator('#setting-embedYouTube').isChecked(), false);
    assert.deepEqual(await page.evaluate(() => ({ writes: storageWrites, saves: saveCalls })), { writes: [], saves: [] });
    await fixture(page, { mobile: !mobile, settings: {}, integratedDefaults: true });
    assert.ok((await categoryState(page)).every(group => group.hidden));
    await page.locator('#settings-expand-all').click();
    assert.equal(await page.locator('#setting-linkify').isChecked(), !mobile);
  });
}
