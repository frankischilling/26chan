import { test, expect } from '@playwright/test';

const catalog = '/test/catalog';
const key = 'catalog-settings';
const lockName = 'paperboard-thread-watcher';
const initial = { orderby: 'alt', large: false, extended: true };
const restored = { orderby: 'date', large: true, extended: true };

async function prepare(page) {
  await page.addInitScript(({ key, initial }) => localStorage.setItem(key, JSON.stringify(initial)), { key, initial });
  await page.goto(catalog);
  await expect(page.locator('script[src="/static/catalog-preferences.v1.js"]')).toHaveAttribute('type', 'module');
  await expect(page.locator('#catalog-preference-status')).toBeHidden();
}

async function holdLock(page) {
  await page.goto('/');
  await page.evaluate(name => new Promise(resolve => {
    window.catalogLockDone = navigator.locks.request(name, async () => {
      resolve();
      await new Promise(release => { window.releaseCatalogLock = release; });
      if (window.catalogLockRaw !== undefined) localStorage.setItem('catalog-settings', window.catalogLockRaw);
    });
  }), lockName);
}

async function releaseLock(page, raw) {
  await page.evaluate(async value => {
    if (value !== undefined) window.catalogLockRaw = value;
    window.releaseCatalogLock();
    await window.catalogLockDone;
  }, raw);
}

const stored = page => page.evaluate(key => JSON.parse(localStorage.getItem(key)), key);

test('held shared lock keeps controls immediate and commits only the latest local catalog choice', async ({ page, context }) => {
  await prepare(page);
  const other = await context.newPage();
  await holdLock(other);
  try {
    await page.locator('#order-ctrl').selectOption('r');
    await page.locator('#size-ctrl').selectOption('large');
    await page.locator('#teaser-ctrl').selectOption('off');
    await expect(page.locator('#order-ctrl')).toHaveValue('r');
    await expect(page.locator('#threads')).toHaveClass('catalog large');
    expect(await stored(page)).toEqual(initial);
    await releaseLock(other);
    await expect.poll(() => stored(page)).toEqual({ orderby: 'r', large: true, extended: false });
    await expect(page.locator('#catalog-preference-status')).toBeHidden();
  } finally { await other.close(); }
});

test('reset updates the catalog before its held-lock preference removal commits', async ({ page, context }) => {
  await prepare(page);
  await page.locator('#order-ctrl').selectOption('r');
  await page.locator('#size-ctrl').selectOption('large');
  await page.locator('#teaser-ctrl').selectOption('off');
  const saved = { orderby: 'r', large: true, extended: false };
  await expect.poll(() => stored(page)).toEqual(saved);
  const other = await context.newPage();
  await holdLock(other);
  try {
    await page.getByRole('link', { name: 'Reset', exact: true }).click();
    await expect(page.locator('#threads')).toHaveClass('catalog extended-small');
    expect(await stored(page)).toEqual(saved);
    await releaseLock(other);
    await expect.poll(() => stored(page)).toBeNull();
    await expect(page.locator('#catalog-preference-status')).toBeHidden();
  } finally { await other.close(); }
});

test('cross-tab restore cancels a queued save without replacing local display; restore event applies in place', async ({ page, context }) => {
  await prepare(page);
  await page.locator('#qf-box').fill('owned search');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  const other = await context.newPage();
  await holdLock(other);
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations++; });
  try {
    await page.locator('#order-ctrl').selectOption('r');
    await releaseLock(other, JSON.stringify(restored));
    await expect.poll(() => stored(page)).toEqual(restored);
    await expect(page.locator('#order-ctrl')).toHaveValue('r');
    await expect(page.locator('#catalog-preference-status')).toContainText('another tab');

    await page.evaluate(() => document.dispatchEvent(new Event('4chanPreferencesRestored')));
    await expect(page.locator('#order-ctrl')).toHaveValue('date');
    await expect(page.locator('#size-ctrl')).toHaveValue('large');
    await expect(page.locator('#teaser-ctrl')).toHaveValue('on');
    await expect(page.locator('#qf-box')).toHaveValue('owned search');
    expect(await stored(page)).toEqual(restored);
    expect(new URL(page.url()).searchParams.get('q')).toBe('owned search');
    expect(navigations).toBe(0);
  } finally { await other.close(); }
});

test('persisted pagehide aborts a queued save and persisted pageshow rearms fresh persistence', async ({ page, context }) => {
  await prepare(page);
  const other = await context.newPage();
  await holdLock(other);
  try {
    await page.locator('#order-ctrl').selectOption('r');
    await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
    await releaseLock(other);
    await page.waitForTimeout(50);
    expect(await stored(page)).toEqual(initial);

    await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
    await page.locator('#size-ctrl').selectOption('large');
    await expect.poll(() => stored(page)).toEqual({ orderby: 'r', large: true, extended: true });
  } finally { await other.close(); }
});
