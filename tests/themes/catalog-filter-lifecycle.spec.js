import { test, expect } from '../helpers/visual-diagnostics.js';

test.use({ javaScriptEnabled: true });
const catalog = '/filterui/catalog';
const key = 'catalog-filters';
const lockName = 'paperboard-thread-watcher';
const initial = { 0: { active: 1, pattern: 'sheet', boards: '', hidden: 1, top: 0 } };
const replacement = { 0: { active: 1, pattern: 'folds', boards: '', hidden: 1, top: 0 } };
async function prepare(page, raw = null) {
  await page.addInitScript(({ key, raw }) => {
    localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
    if (raw === null) localStorage.removeItem(key); else localStorage.setItem(key, raw);
  }, { key, raw });
  await page.goto(catalog);
  await expect(page.locator('#filters-ctrl')).toBeVisible();
}
const stored = page => page.evaluate(key => localStorage.getItem(key), key);
const ids = page => page.locator('#threads > .thread').evaluateAll(nodes => nodes.map(node => node.id));
async function openRule(page, pattern = 'folds') {
  await page.locator('#filters-ctrl').click();
  if (!await page.locator('#filters .filter-pattern').count()) await page.locator('#filters-add').click();
  await page.locator('#filters .filter-pattern').first().fill(pattern);
  await page.locator('#filters .filter-hide').first().check();
}
async function holdLock(other) {
  await other.goto('/');
  await other.evaluate(name => new Promise(resolve => {
    window.filterLockDone = navigator.locks.request(name, async () => {
      resolve(); await new Promise(release => { window.releaseFilterLock = release; });
    });
  }), lockName);
}
async function releaseLock(other) {
  await other.evaluate(async () => { window.releaseFilterLock(); await window.filterLockDone; });
}
async function queued(page) {
  await expect.poll(() => page.evaluate(async name => (await navigator.locks.query()).pending.filter(lock => lock.name === name).length, lockName)).toBe(1);
}

test('invalid stored patterns and colors leave all cards visible without resource requests', async ({ page, context }) => {
  const requests = [], errors = [];
  context.on('request', request => requests.push(request.url())); page.on('pageerror', error => errors.push(error.message));
  await prepare(page, JSON.stringify({ 0: { ...initial[0], pattern: '/[/' } }));
  await expect(page.locator('.catalogFilterNotice')).toContainText('could not be checked');
  expect(await ids(page)).toEqual(['thread-1000001', 'thread-1000002', 'thread-1000003']);
  const other = await context.newPage();
  other.on('pageerror', error => errors.push(error.message));
  try {
    await prepare(other, JSON.stringify({ 0: { ...initial[0], color: 'red; background:url(https://invalid.example/owned)' } }));
    await expect(other.locator('.catalogFilterNotice')).toContainText('invalid');
    expect(await ids(other)).toHaveLength(3);
  } finally { await other.close(); }
  expect(requests.some(url => url.includes('invalid.example'))).toBe(false); expect(errors).toEqual([]);
});
test('invalid edited patterns do not replace stored rules or current effects', async ({ page }) => {
  await prepare(page, JSON.stringify(initial));
  await expect.poll(() => ids(page)).toEqual(['thread-1000003', 'thread-1000002']);
  await openRule(page, '/[/'); await page.locator('#filters-save').click();
  await expect(page.locator('#filters-msg')).toContainText('not saved');
  expect(await stored(page)).toBe(JSON.stringify(initial));
  expect(await ids(page)).toEqual(['thread-1000003', 'thread-1000002']);
});
test('a held shared lock commits a reviewed catalog filter once released', async ({ page, context }) => {
  await prepare(page, JSON.stringify(initial)); await openRule(page);
  const other = await context.newPage(); await holdLock(other);
  try {
    await page.locator('#filters-save').click(); await queued(page);
    expect(await stored(page)).toBe(JSON.stringify(initial)); await releaseLock(other);
    await expect(page.locator('#filters')).not.toBeVisible();
    expect(await stored(page)).toBe(JSON.stringify(replacement));
    expect(await ids(page)).toEqual(['thread-1000003', 'thread-1000001']);
  } finally { await other.close(); }
});
test('a cross-tab change cancels a queued save and cannot be overwritten by stale edits', async ({ page, context }) => {
  await prepare(page, JSON.stringify(initial)); await openRule(page, 'feeling');
  const other = await context.newPage(); await holdLock(other);
  try {
    await page.locator('#filters-save').click(); await queued(page);
    await other.evaluate(({ key, replacement }) => localStorage.setItem(key, JSON.stringify(replacement)), { key, replacement });
    await expect.poll(() => ids(page)).toEqual(['thread-1000003', 'thread-1000001']);
    await expect(page.locator('#filters-msg')).toContainText('another tab');
    await releaseLock(other); await expect(page.locator('#filters-save')).toBeEnabled();
    await page.locator('#filters-save').click(); await expect(page.locator('#filters-msg')).toContainText('another tab');
    expect(await stored(page)).toBe(JSON.stringify(replacement));
  } finally { await other.close(); }
});
for (const [label, retire] of [
  ['close', () => document.getElementById('filters-close').click()],
  ['detachment', () => document.getElementById('ctrl').remove()],
  ['BFCache suspension', () => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }))],
]) {
  test(`${label} cancels a queued catalog save before the lock is released`, async ({ page, context }) => {
    await prepare(page, JSON.stringify(initial)); await openRule(page);
    const other = await context.newPage(); await holdLock(other);
    try {
      await page.locator('#filters-save').click(); await queued(page); await page.evaluate(retire);
      await releaseLock(other); expect(await stored(page)).toBe(JSON.stringify(initial));
      await expect(page.locator('#filters')).not.toBeVisible();
      if (label === 'BFCache suspension') {
        await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
        await expect.poll(() => ids(page)).toEqual(['thread-1000003', 'thread-1000002']);
      }
    } finally { await other.close(); }
  });
}
for (const [label, setup] of [
  ['no Web Locks', () => Object.defineProperty(navigator, 'locks', { value: undefined })],
  ['denied Web Locks', () => { navigator.locks.request = async () => { throw new DOMException('Owned denial', 'SecurityError'); }; }],
  ['denied writes', () => {
    const original = Storage.prototype.setItem;
    Storage.prototype.setItem = function(key, value) { if (key === 'catalog-filters') throw new DOMException('Owned denial', 'SecurityError'); return original.call(this, key, value); };
  }],
]) {
  test(`${label} keeps edited catalog filters in the tab without claiming persistence`, async ({ page }) => {
    await page.addInitScript(setup); await prepare(page); await openRule(page);
    await page.locator('#filters-save').click(); await expect(page.locator('#filters-msg')).toContainText('only in this tab');
    expect(await stored(page)).toBeNull();
    expect(await ids(page)).toEqual(['thread-1000003', 'thread-1000001']);
  });
}
