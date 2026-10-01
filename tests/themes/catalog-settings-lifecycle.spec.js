import { watcherSettingsOpener } from '../browser/helpers/watcher-settings.js';
import { test, expect } from '../helpers/visual-diagnostics.js';

test.use({ javaScriptEnabled: true });
const catalog = '/settingsui/catalog', themeKey = 'catalog-theme', settingsKey = '4chan-settings';
const lockName = 'paperboard-thread-watcher';
const initial = { newtab: true, css: '.teaser { color: #008000; }' };
const errors = new WeakMap(), requests = new WeakMap();
test.beforeEach(({ page }) => {
  errors.set(page, []); requests.set(page, []);
  page.on('pageerror', error => errors.get(page).push(error.message));
  page.on('request', request => requests.get(page).push(request.url()));
});
test.afterEach(({ page }) => {
  expect(errors.get(page)).toEqual([]);
  expect(requests.get(page).filter(url => new URL(url).origin !== 'http://127.0.0.1:3000')).toEqual([]);
});
async function prepare(page, { theme = initial, settings = { disableAll: true }, failure = null } = {}) {
  await page.addInitScript(({ theme, settings, failure }) => {
    const set = Storage.prototype.setItem, remove = Storage.prototype.removeItem, get = Storage.prototype.getItem;
    window.ownedStoredSettings = () => ({ theme: get.call(localStorage, 'catalog-theme'), settings: get.call(localStorage, '4chan-settings') });
    for (const [key, value] of Object.entries({ 'catalog-theme': theme, '4chan-settings': settings })) {
      if (value === null) remove.call(localStorage, key); else set.call(localStorage, key, JSON.stringify(value));
    }
    if (failure === 'no Web Locks') Object.defineProperty(navigator, 'locks', { value: undefined });
    else if (failure === 'denied Web Locks') navigator.locks.request = async () => { throw new DOMException('Owned denial', 'SecurityError'); };
    else if (failure === 'denied storage access') {
      for (const method of ['getItem', 'setItem', 'removeItem']) Storage.prototype[method] = function() { throw new DOMException('Owned denial', 'SecurityError'); };
    }
    else if (failure === 'denied theme writes' || failure === 'denied native writes') {
      Storage.prototype.setItem = function(key, value) {
        if (key === (failure === 'denied theme writes' ? 'catalog-theme' : '4chan-settings')) throw new DOMException('Owned denial', 'SecurityError');
        return set.call(this, key, value);
      };
    }
  }, { theme, settings, failure });
  await page.goto(catalog); await expect(page.locator('#qf-ctrl')).toBeVisible();
}
const stored = page => page.evaluate(() => window.ownedStoredSettings());
async function edit(page) {
  await watcherSettingsOpener(page).click(); await expect(page.locator('#theme')).toBeVisible();
  await page.locator('#theme-nobinds').check(); await page.locator('#theme-css').fill('.teaser { color: #ff0000; }');
}
async function hold(other) {
  await other.goto('/');
  await other.evaluate(name => new Promise(resolve => {
    window.settingsLockDone = navigator.locks.request(name, async () => {
      resolve(); await new Promise(release => { window.releaseSettingsLock = release; });
    });
  }), lockName);
}
async function release(other) { await other.evaluate(async () => { window.releaseSettingsLock(); await window.settingsLockDone; }); }
async function queued(page) {
  await expect.poll(() => page.evaluate(async name => (await navigator.locks.query()).pending.filter(lock => lock.name === name).length, lockName)).toBe(1);
}
test('a held shared lock saves both reviewed keys after release without a page navigation', async ({ page, context }) => {
  await prepare(page); await edit(page); const before = await stored(page);
  const other = await context.newPage(); await hold(other);
  let navigations = 0; page.on('framenavigated', () => navigations++);
  try {
    await page.locator('#theme-save').click(); await queued(page); expect(await stored(page)).toEqual(before);
    await release(other); await expect(page.locator('#theme')).toBeHidden();
    expect(JSON.parse((await stored(page)).theme)).toEqual({ nobinds: true, newtab: true, css: '.teaser { color: #ff0000; }' });
    expect(JSON.parse((await stored(page)).settings)).toEqual({ disableAll: true, threadWatcher: false, dropDownNav: false });
    await expect(page.locator('#threads .teaser').first()).toHaveCSS('color', 'rgb(255, 0, 0)'); expect(navigations).toBe(0);
  } finally { await other.close(); }
});
for (const key of [themeKey, settingsKey]) {
  test(`a competing ${key} edit cancels a queued save and rejects the stale editor`, async ({ page, context }) => {
    await prepare(page); await edit(page); const other = await context.newPage(); await hold(other);
    const replacement = JSON.stringify(key === themeKey ? { nospoiler: true } : { disableAll: true, unrelated: 'newer' });
    try {
      await page.locator('#theme-save').click(); await queued(page);
      await other.evaluate(({ key, replacement }) => localStorage.setItem(key, replacement), { key, replacement });
      await expect(page.locator('#theme-msg')).toContainText('another tab'); await release(other);
      await expect(page.locator('#theme-save')).toBeEnabled(); await page.locator('#theme-save').click();
      await expect(page.locator('#theme-msg')).toContainText('another tab');
      expect(await page.evaluate(key => localStorage.getItem(key), key)).toBe(replacement);
    } finally { await other.close(); }
  });
}
for (const [label, retire] of [
  ['close', () => document.getElementById('theme-close').click()],
  ['detachment', () => document.getElementById('ctrl').remove()],
  ['BFCache suspension', () => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }))],
]) {
  test(`${label} cancels a queued Settings save before lock entry`, async ({ page, context }) => {
    await prepare(page); await edit(page); const before = await stored(page);
    const other = await context.newPage(); await hold(other);
    try {
      await page.locator('#theme-save').click(); await queued(page); await page.evaluate(retire);
      if (label !== 'close') expect(await page.evaluate(() => document.adoptedStyleSheets.length)).toBe(0);
      await release(other);
      expect(await stored(page)).toEqual(before); await expect(page.locator('#theme')).toBeHidden();
      if (label === 'BFCache suspension') {
        await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
        await expect(page.locator('#threads .teaser').first()).toHaveCSS('color', 'rgb(0, 128, 0)');
        await watcherSettingsOpener(page).click(); await expect(page.locator('#theme')).toBeVisible();
      }
    } finally { await other.close(); }
  });
}
for (const failure of ['no Web Locks', 'denied Web Locks', 'denied storage access', 'denied theme writes', 'denied native writes']) {
  test(`${failure} applies reviewed Settings only in this tab and preserves both stored keys`, async ({ page }) => {
    await prepare(page, { failure }); await edit(page); const before = await stored(page);
    await page.locator('#theme-save').click(); await expect(page.locator('#theme-msg')).toContainText('only in this tab');
    expect(await stored(page)).toEqual(before);
    await expect(page.locator('#threads .teaser').first()).toHaveCSS('color', 'rgb(255, 0, 0)');
    await page.locator('#theme-close').click(); await watcherSettingsOpener(page).click();
    await expect(page.locator('#theme-nobinds')).toBeChecked(); await expect(page.locator('#theme-css')).toHaveValue('.teaser { color: #ff0000; }');
  });
}
test('an incomplete rollback is reported and a replacement from another writer is preserved', async ({ page }) => {
  await prepare(page); await edit(page); const before = await stored(page);
  const newer = '{"newtab":true,"nospoiler":true}';
  await page.evaluate(newer => {
    const set = Storage.prototype.setItem;
    Storage.prototype.setItem = function(key, value) {
      if (key === '4chan-settings') {
        set.call(this, 'catalog-theme', newer); throw new DOMException('Owned competing writer', 'QuotaExceededError');
      }
      return set.call(this, key, value);
    };
  }, newer);
  await page.locator('#theme-save').click(); await expect(page.locator('#theme-msg')).toContainText('could not be recovered');
  expect(await stored(page)).toEqual({ theme: newer, settings: before.settings });
  await expect(page.locator('#theme')).toBeVisible(); await expect(page.locator('body')).toHaveClass(/reveal-img-spoilers/);
});
test('unsafe stored CSS stays inert while valid flags work; unsafe edited CSS cannot replace storage', async ({ page }) => {
  const theme = { nospoiler: true, newtab: true, css: '.teaser { background-image: url(https://example.invalid/owned); } body { display: none; }' };
  await prepare(page, { theme }); await expect(page.locator('body')).toHaveClass(/reveal-img-spoilers/);
  await expect(page.locator('#threads .catalogThumb').first()).toHaveAttribute('target', '_blank');
  await watcherSettingsOpener(page).click(); await expect(page.locator('#theme-msg')).toContainText('could not be applied');
  await expect(page.locator('#theme-css')).toHaveValue(theme.css); const before = await stored(page);
  await page.locator('#theme-save').click(); await expect(page.locator('#theme-msg')).toHaveText('CSS functions are not allowed.');
  expect(await stored(page)).toEqual(before); await expect(page.locator('#theme')).toBeVisible();
  expect(await page.evaluate(() => document.adoptedStyleSheets.length)).toBe(0);
});

for (const [path, links] of [[catalog, '#threads .catalogThumb'], ['/settingstext/catalog', '#threads .txt-sub a']]) {
  test(`${path} applies stored new-tab flags before the Settings entry point is available`, async ({ page, visualDiagnostics }) => {
    const theme = { newtab: true, nospoiler: true, css: '.teaser { background-image: url(https://example.invalid/owned); }' };
    await page.addInitScript(theme => {
      localStorage.setItem('catalog-theme', JSON.stringify(theme));
      localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
    }, theme);
    await page.route('**/static/thread-watcher.v1.js', route => route.abort('connectionfailed'));
    await page.goto(path); await expect(page.locator('#qf-ctrl')).toBeVisible();
    await expect(page.locator('#settingsWindowLink')).not.toHaveAttribute('data-native-settings-ready', '');
    await expect(page.locator('#settingsWindowLink')).toHaveAttribute('href', /\/settings\/theme\?worksafe=true$/);
    expect(visualDiagnostics.failedScripts).toEqual([{ path: '/static/thread-watcher.v1.js', error: 'net::ERR_CONNECTION_FAILED' }]);
    await expect(page.locator(links).first()).toHaveAttribute('target', '_blank');
    for (const link of await page.locator(links).all()) {
      await expect(link).toHaveAttribute('target', '_blank');
      await expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    }
    await expect(page.locator('body')).toHaveClass(/reveal-img-spoilers/);
    expect(await page.evaluate(() => document.adoptedStyleSheets.length)).toBe(0);
    expect(JSON.parse(await page.evaluate(() => localStorage.getItem('catalog-theme')))).toEqual(theme);
    // An explicit fresh navigation, after restoring the owned script, must also
    // restore the actual Settings UI without any automatic transport retry.
    await page.unroute('**/static/thread-watcher.v1.js'); await page.reload();
    await expect(page.locator('#settingsWindowLink')).toHaveAttribute('data-native-settings-ready', '');
    await watcherSettingsOpener(page).click(); await expect(page.locator('#theme')).toBeVisible();
    await expect(page.locator('#theme-newtab')).toBeChecked();
    await expect(page.locator('#theme-msg')).toContainText('could not be applied');
    await expect(page.locator(links).first()).toHaveAttribute('target', '_blank');
  });
}
for (const [path, linkSelector] of [[catalog, '.catalogThumb'], ['/settingstext/catalog', '.txt-sub a']]) {
  for (const rejected of ['http://[', 'https://foreign.invalid/owned']) {
    test(`${path} resolves inert canonical links and skips missing or rejected href ${rejected}`, async ({ page }) => {
      await prepare(page, { theme: { newtab: true } });
      if (path !== catalog) await page.goto(path);
      const evidence = await page.evaluate(async ({ linkSelector, rejected }) => {
        const form = document.getElementById('ctrl').cloneNode(true);
        const container = document.getElementById('threads');
        const hidden = document.getElementById('catalogFiltered');
        const cards = [...container.querySelectorAll('.thread')];
        if (cards.length < 3) throw new Error('Owned fixture requires three cards.');
        const [missing, invalid, canonical] = cards.slice(0, 3);
        const missingLink = missing.querySelector(linkSelector), invalidLink = invalid.querySelector(linkSelector);
        missingLink.removeAttribute('href'); invalidLink.setAttribute('href', rejected);
        for (const link of [missingLink, invalidLink]) { link.target = '_self'; link.rel = 'author'; }
        const canonicalLink = canonical.querySelector(linkSelector);
        canonicalLink.removeAttribute('target'); canonicalLink.removeAttribute('rel');
        if (canonical.localName === 'tr') {
          const table = document.createElement('table'), rows = document.createElement('tbody');
          rows.append(canonical); table.append(rows); hidden.content.replaceChildren(table);
        } else hidden.content.replaceChildren(canonical);
        const evidence = { missing: missing.id, invalid: invalid.id, canonical: canonical.id,
          href: canonicalLink.getAttribute('href'), base: canonicalLink.ownerDocument.baseURI };
        document.body.replaceChildren(form, container, hidden);
        await new Promise((resolve, reject) => {
          const script = document.createElement('script'); script.type = 'module';
          script.src = '/static/catalog-preferences.v1.js?owned-inert-links=1';
          script.onload = resolve; script.onerror = reject; document.body.append(script);
        });
        return evidence;
      }, { linkSelector, rejected });
      expect(evidence.base).toBe('about:blank');
      expect(evidence.href).toMatch(/^\/[a-z0-9]+\/thread\/[0-9]+$/);
      expect(await page.evaluate(({ id, selector }) => {
        const link = document.getElementById('catalogFiltered').content.querySelector(`#${id} ${selector}`);
        return { target: link.getAttribute('target'), rel: link.getAttribute('rel') };
      }, { id: evidence.canonical, selector: linkSelector })).toEqual({ target: '_blank', rel: 'noopener noreferrer' });
      await page.locator('#qf-ctrl').click(); await expect(page.locator('#qf-box')).toBeVisible();
      await page.locator('#qf-box').press('Enter');
      const canonical = page.locator(`#${evidence.canonical} ${linkSelector}`);
      await expect(canonical).toHaveAttribute('target', '_blank');
      await expect(canonical).toHaveAttribute('rel', 'noopener noreferrer');
      for (const id of [evidence.missing, evidence.invalid]) {
        const link = page.locator(`#${id} ${linkSelector}`);
        await expect(link).toHaveAttribute('target', '_self'); await expect(link).toHaveAttribute('rel', 'author');
      }
      expect(JSON.parse(await page.evaluate(() => localStorage.getItem('catalog-theme')))).toEqual({ newtab: true });
    });
  }
}
test('the GET spoiler control and Reset preserve other catalog theme fields under the shared lock', async ({ page, context }) => {
  await prepare(page); const before = (await stored(page)).theme;
  const other = await context.newPage(); await hold(other);
  try {
    await page.locator('#catalog-spoilers').selectOption('on'); await queued(page);
    expect((await stored(page)).theme).toBe(before); await release(other);
    await expect.poll(async () => JSON.parse((await stored(page)).theme)).toEqual({ ...initial, nospoiler: true });
    await page.locator('#catalog-reset').click(); await expect.poll(async () => JSON.parse((await stored(page)).theme)).toEqual(initial);
    await expect(page.locator('#threads .teaser').first()).toHaveCSS('color', 'rgb(0, 128, 0)');
  } finally { await other.close(); }
});
test('queued spoiler changes are cancelled on page suspension', async ({ page, context }) => {
  await prepare(page); const before = await stored(page); const other = await context.newPage(); await hold(other);
  try {
    await page.locator('#catalog-spoilers').selectOption('on'); await queued(page);
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
    await release(other); expect(await stored(page)).toEqual(before);
  } finally { await other.close(); }
});
test('catalog watcher and drop-down options apply and clear the actual mounted controls without reload', async ({ page }) => {
  await prepare(page, { theme: null }); await edit(page);
  let navigations = 0; page.on('framenavigated', () => navigations++);
  await page.locator('#theme-tw').check(); await page.locator('#theme-ddn').check(); await page.locator('#theme-save').click();
  await expect(page.locator('#theme')).toBeHidden(); await expect(page.locator('#threadWatcher')).toBeVisible();
  const bar = page.getByRole('navigation', { name: 'Persistent board navigation', exact: true }); await expect(bar).toBeVisible();
  await bar.getByRole('button', { name: 'Edit boards', exact: true }).click();
  const boardList = page.getByRole('dialog', { name: 'Custom Board List', exact: true }); await expect(boardList).toBeVisible();
  await boardList.getByLabel('Boards', { exact: true }).fill('settingsui demo');
  await boardList.getByRole('button', { name: 'Save board list', exact: true }).click(); await expect(boardList).toHaveCount(0);
  expect(await bar.locator('option').evaluateAll(options => options.map(option => option.value))).toEqual(['demo', 'f', 'settingsui', 'zed']);
  expect(await bar.locator('.nativeCustomBoardLinks a').evaluateAll(links => links.map(link => link.getAttribute('href')))).toEqual(['/settingsui/', '/demo/']);
  expect(JSON.parse((await stored(page)).settings).customMenuList).toBe('settingsui demo');
  await bar.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.locator('#theme-tw').uncheck(); await page.locator('#theme-ddn').uncheck(); await page.locator('#theme-save').click();
  await expect(page.locator('#threadWatcher')).toBeHidden(); await expect(bar).toHaveCount(0); expect(navigations).toBe(0);
});
