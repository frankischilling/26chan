import { test, expect } from '@playwright/test';

async function listen(page, options = {}) {
  await page.addInitScript(options => {
    window.catalogInitTrace = [];
    window.catalogControlsBound = false;
    const addListener = EventTarget.prototype.addEventListener;
    EventTarget.prototype.addEventListener = function(type, ...args) {
      const result = addListener.call(this, type, ...args);
      if (this.id === 'teaser-ctrl' && type === 'change') {
        catalogControlsBound = true; EventTarget.prototype.addEventListener = addListener;
      }
      return result;
    };
    if (options.saved !== undefined) localStorage.setItem('catalog-settings', options.saved);
    if (options.disabled) localStorage.setItem('4chan-settings', '{"disableAll":true}');
    sessionStorage.setItem('4chan-catalog-search', 'source-init-search');
    sessionStorage.setItem('4chan-catalog-search-board', options.board ?? 'fixture');
    if (options.noSession) { sessionStorage.removeItem('4chan-catalog-search'); sessionStorage.removeItem('4chan-catalog-search-board'); }
    if (options.blocked) Object.defineProperty(Storage.prototype, 'getItem', { value() { throw new DOMException('blocked', 'SecurityError'); } });
    if (options.empty) document.addEventListener('readystatechange', () => {
      if (document.readyState !== 'interactive') return;
      document.querySelectorAll('#threads .thread').forEach(node => node.remove());
      document.getElementById('catalogFiltered')?.content.replaceChildren();
    });
    document.addEventListener('4chanMainInit', event => {
      const root = document.getElementById('threads');
      catalogInitTrace.push({ type: event.constructor.name, target: event.target === document,
        bubbles: event.bubbles, cancelable: event.cancelable, detail: Object.hasOwn(event, 'detail'),
        order: document.getElementById('order-ctrl').value, size: document.getElementById('size-ctrl').value,
        teaser: document.getElementById('teaser-ctrl').value, className: root.className,
        query: document.getElementById('qf-box').value,
        menus: root.querySelectorAll('.postMenuBtn').length,
        filters: document.querySelectorAll('.catalogFilterNotice').length,
      });
      if (options.changeSpoilersDuringInit) {
        window.catalogRenderBatches = 0;
        new MutationObserver(() => catalogRenderBatches++).observe(root, { childList: true });
        const control = document.getElementById('catalog-spoilers');
        control.value = 'on'; control.dispatchEvent(new Event('change', { bubbles: true }));
      }
      if (options.changeDuringInit) {
        for (const [id, value] of [['order-ctrl', 'r'], ['size-ctrl', 'large'], ['teaser-ctrl', 'off']]) {
          const control = document.getElementById(id); control.value = value; control.dispatchEvent(new Event('change', { bubbles: true }));
        }
      }
      if (options.interrupt) window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: options.interrupt !== 'depart' }));
      if (options.interrupt === 'replace') root.replaceWith(root.cloneNode(true));
    });
  }, options);
}
const saved = JSON.stringify({ orderby: 'r', large: true, extended: false });
const cases = [
  ['default', '/fixture/catalog', {}, ['alt', 'small', 'on', 'catalog extended-small']],
  ['saved', '/fixture/catalog', { saved }, ['r', 'large', 'off', 'catalog large']],
  ['URL override', '/fixture/catalog?order=date&size=small&teaser=on', { saved }, ['date', 'small', 'on', 'catalog extended-small']],
  ['malformed storage', '/fixture/catalog', { saved: '{' }, ['alt', 'small', 'on', 'catalog extended-small']],
  ['blocked storage', '/fixture/catalog', { blocked: true }, ['alt', 'small', 'on', 'catalog extended-small']],
  ['disabled extension', '/fixture/catalog', { disabled: true }, ['alt', 'small', 'on', 'catalog extended-small']],
  ['empty catalog', '/fixture/catalog', { empty: true }, ['alt', 'small', 'on', 'catalog extended-small']],
  ['text catalog', '/news/catalog', { board: 'news' }, ['alt', 'small', 'on', 'catalog textCatalog']],
];
for (const [name, path, options, display] of cases) test(`catalog MainInit observes controls before native load: ${name}`, async ({ page }) => {
  await listen(page, options); await page.goto(path);
  await expect.poll(() => page.evaluate(() => catalogInitTrace.length)).toBe(1);
  expect(await page.evaluate(() => catalogInitTrace[0])).toEqual({
    type: 'Event', target: true, bubbles: false, cancelable: false, detail: false,
    order: display[0], size: display[1], teaser: display[2], className: display[3], query: '', menus: 0, filters: 0,
  });
  if (!options.blocked) await expect(page.locator('#qf-box')).toHaveValue('source-init-search');
  if (options.empty) await expect(page.locator('#threads .thread')).toHaveCount(0);
  await page.locator('#order-ctrl').selectOption('absdate');
  await page.getByRole('link', { name: 'Reset', exact: true }).click();
  await page.evaluate(() => {
    document.dispatchEvent(new CustomEvent('4chanPreferencesRestored', { detail: { keys: ['catalog-settings'] } }));
    document.dispatchEvent(new Event('4chanSettingsSaved'));
    window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
    window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
  });
  expect(await page.evaluate(() => catalogInitTrace.length)).toBe(1);
});
for (const interrupt of ['resume', 'depart', 'replace']) test(`catalog MainInit listener interruption: ${interrupt}`, async ({ page }) => {
  await listen(page, { interrupt }); await page.goto('/fixture/catalog');
  await expect.poll(() => page.evaluate(() => catalogInitTrace.length)).toBe(1);
  await expect(page.locator('#qf-box')).toHaveValue('');
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  if (interrupt === 'resume') await expect(page.locator('#qf-box')).toHaveValue('source-init-search');
  else await expect(page.locator('#qf-box')).toHaveValue('');
  expect(await page.evaluate(() => catalogInitTrace.length)).toBe(1);
});

test('a delayed watcher cannot be replaced by forged readiness events', async ({ page }) => {
  let release; const pending = new Promise(resolve => { release = resolve; });
  await page.route('**/static/thread-watcher.v1.js', async route => { await pending; await route.continue(); });
  await listen(page);
  const navigation = page.goto('/fixture/catalog');
  await expect(page.locator('#ctrl')).toBeAttached();
  await page.waitForFunction(() => catalogControlsBound);
  await expect(page.locator('.catalogApplyFallback')).toBeVisible();
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  await page.locator('#order-ctrl').selectOption('r');
  await page.locator('#size-ctrl').selectOption('large');
  await page.locator('#teaser-ctrl').selectOption('off');
  await page.locator('#qf-box').fill('early search choice');
  expect(await page.evaluate(() => localStorage.getItem('catalog-settings'))).toBeNull();
  await page.evaluate(() => {
    for (const name of ['4chanSettingsSaved', '4chanPreferencesRestored', '4chanCatalogThemeApplied']) document.dispatchEvent(new Event(name));
    window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
  });
  expect(await page.evaluate(() => catalogInitTrace)).toEqual([]);
  release(); await navigation;
  expect(await page.evaluate(() => catalogInitTrace)).toEqual([]);
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  await expect.poll(() => page.evaluate(() => catalogInitTrace.length)).toBe(1);
  await expect(page.locator('#qf-box')).toHaveValue('early search choice');
  await expect(page.locator('#threads')).toHaveClass('catalog large');
  expect(await page.evaluate(() => catalogInitTrace[0].order)).toBe('r');
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('catalog-settings')))).toEqual({ orderby: 'r', large: true, extended: false });
  expect(errors).toEqual([]);
});

test('an unavailable essential catalog module leaves server-rendered GET controls without a false event', async ({ page }) => {
  await listen(page); await page.route('**/static/catalog-theme.v1.js', route => route.abort());
  await page.goto('/fixture/catalog');
  await expect(page.locator('#ctrl')).not.toHaveClass(/nativeCatalogControls/);
  await expect(page.locator('#threads')).toBeVisible();
  expect(await page.evaluate(() => catalogInitTrace)).toEqual([]);
});

for (const interrupted of [false, true]) test(`synchronous MainInit control changes queue until catalog loading (suspended=${interrupted})`, async ({ page }) => {
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  await listen(page, { changeDuringInit: true, ...(interrupted ? { interrupt: 'resume' } : {}) });
  await page.goto('/fixture/catalog');
  await expect.poll(() => page.evaluate(() => catalogInitTrace.length)).toBe(1);
  expect(await page.evaluate(() => catalogInitTrace[0].className)).toBe('catalog extended-small');
  if (interrupted) {
    expect(await page.evaluate(() => localStorage.getItem('catalog-settings'))).toBeNull();
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  }
  await expect(page.locator('#threads')).toHaveClass('catalog large');
  await expect(page.locator('#order-ctrl')).toHaveValue('r');
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('catalog-settings')))).toEqual({ orderby: 'r', large: true, extended: false });
  expect(await page.evaluate(() => catalogInitTrace.length)).toBe(1);
  expect(errors).toEqual([]);
});

test('a failed optional watcher leaves native catalog display, Enter search and Reset available', async ({ page }) => {
  await listen(page, { noSession: true }); await page.route('**/static/thread-watcher.v1.js', route => route.abort());
  await page.goto('/fixture/catalog'); await page.waitForFunction(() => catalogControlsBound);
  await expect(page.locator('#ctrl')).toHaveClass(/nativeCatalogControls/);
  await expect(page.locator('#qf-ctrl')).toBeVisible();
  await expect.poll(() => page.evaluate(() => catalogInitTrace.length)).toBe(1);
  const initialCount = await page.locator('#threads > .thread').count();
  await page.locator('#order-ctrl').selectOption('r');
  await page.locator('#qf-ctrl').click();
  await page.locator('#qf-box').fill('fallback query');
  await page.locator('#qf-box').press('Enter');
  await expect(page).toHaveURL(/order=r/); await expect(page).toHaveURL(/q=fallback/);
  await expect(page.locator('#threads > .thread')).toHaveCount(0);
  await page.locator('#qf-box').fill('enter query'); await page.locator('#qf-box').press('Enter');
  await expect(page).toHaveURL(/q=enter/);
  await page.getByRole('link', { name: 'Reset', exact: true }).click();
  await expect(page).toHaveURL(/\/fixture\/catalog\?order=alt&size=small&teaser=on$/);
  await expect(page.locator('#threads > .thread')).toHaveCount(initialCount);
  await expect(page.locator('#qf-box')).toHaveValue('');
  expect(await page.evaluate(() => catalogInitTrace.length)).toBe(1);
});

test('a MainInit spoiler choice renders without a session query or another display change', async ({ page }) => {
  await listen(page, { noSession: true, changeSpoilersDuringInit: true });
  await page.goto('/fixture/catalog');
  await expect.poll(() => page.evaluate(() => catalogInitTrace.length)).toBe(1);
  await expect.poll(() => page.evaluate(() => catalogRenderBatches)).toBeGreaterThan(0);
  await expect(page.locator('#qf-box')).toHaveValue('');
  await expect(page.locator('#catalog-spoilers')).toHaveValue('on');
  expect(await page.evaluate(() => Array.from(document.querySelectorAll('#threads img[data-spoiler-src]'))
    .every(image => image.src === image.dataset.spoilerSrc))).toBe(true);
});

test('an explicitly cleared pending search removes the previous session query', async ({ page }) => {
  let release; const pending = new Promise(resolve => { release = resolve; });
  await page.route('**/static/thread-watcher.v1.js', async route => { await pending; await route.continue(); });
  await listen(page); const navigation = page.goto('/fixture/catalog');
  await page.waitForFunction(() => catalogControlsBound);
  await page.locator('#qf-box').fill('temporary input'); await page.locator('#qf-box').fill('');
  release(); await navigation;
  await expect.poll(() => page.evaluate(() => catalogInitTrace.length)).toBe(1);
  await expect(page.locator('#qf-box')).toHaveValue('');
  expect(await page.evaluate(() => sessionStorage.getItem('4chan-catalog-search'))).toBeNull();
});
