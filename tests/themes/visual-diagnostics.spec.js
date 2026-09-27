import { test, expect, readVisualState } from '../helpers/visual-diagnostics.js';
test.use({ javaScriptEnabled: true });

async function holdScript(page, pattern) {
  let release;
  let intercepted;
  const reached = new Promise(resolve => { intercepted = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  await page.route(pattern, async route => {
    intercepted();
    await gate;
    await route.continue();
  });
  return { reached, release, dispose: () => page.unroute(pattern) };
}

test('navigation waits for delayed catalog and board scripts before exposing their controls', async ({ page }) => {
  const catalog = await holdScript(page, '**/static/catalog-preferences.v1.js');
  let catalogSettled = false;
  const catalogNavigation = page.goto('/img/catalog?size=small&spoilers=off&q=').finally(() => { catalogSettled = true; });
  await catalog.reached;
  expect(catalogSettled).toBe(false);
  catalog.release();
  await catalogNavigation;
  await catalog.dispose();
  const spoiler = page.locator('#threads img[data-spoiler-src]').first();
  const source = await spoiler.getAttribute('data-spoiler-src');
  await page.getByLabel('Spoilers:', { exact: true }).selectOption('on');
  await expect(spoiler).toHaveAttribute('src', source);

  const watcher = await holdScript(page, '**/static/thread-watcher.v1.js');
  let watcherSettled = false;
  const watcherNavigation = page.goto('/demo/').finally(() => { watcherSettled = true; });
  await watcher.reached;
  expect(watcherSettled).toBe(false);
  watcher.release();
  await watcherNavigation;
  await watcher.dispose();
  await expect(page.locator('.postMenuBtn').first()).toBeVisible();
  await page.locator('.postInfo > .postNum').first().click();
  await expect(page.locator('#quickReply')).toBeVisible();
});

test('page-script transport failures reproduce inert controls and retain bounded network evidence', async ({ page, visualDiagnostics }) => {
  await page.route('**/static/catalog-preferences.v1.js', route => route.abort('connectionfailed'));
  await page.goto('/img/catalog?size=small&spoilers=off&q=');
  const spoiler = page.locator('#threads img[data-spoiler-src]').first();
  await expect(spoiler).toHaveAttribute('src', '/static/catalog/spoiler.png');
  await page.getByLabel('Spoilers:', { exact: true }).selectOption('on');
  await expect(page.getByLabel('Spoilers:', { exact: true })).toHaveValue('on');
  await expect(spoiler).toHaveAttribute('src', '/static/catalog/spoiler.png');
  await page.unroute('**/static/catalog-preferences.v1.js');

  await page.route('**/static/thread-watcher.v1.js', route => route.abort('connectionfailed'));
  await page.goto('/demo/');
  await expect(page.locator('.postMenuBtn')).toHaveCount(0);
  await expect(page.locator('#quickReply')).toHaveCount(0);
  expect(visualDiagnostics.failedScripts).toEqual([
    { path: '/static/catalog-preferences.v1.js', error: 'net::ERR_CONNECTION_FAILED' },
    { path: '/static/thread-watcher.v1.js', error: 'net::ERR_CONNECTION_FAILED' },
  ]);
});

test('script transport failures retain only a bounded path and network error code', async ({ page, visualDiagnostics }) => {
  await page.goto('/demo/');
  await expect(page.locator('.postMenuBtn').first()).toBeVisible();
  await page.route('**/static/thread-watcher.v1.js?*', route => route.abort('connectionfailed'));
  await page.evaluate(() => new Promise(resolve => {
    const script = document.createElement('script');
    script.type = 'module';
    script.src = '/static/thread-watcher.v1.js?private=QUERY_SENTINEL#FRAGMENT_SENTINEL';
    script.onerror = () => resolve();
    document.head.append(script);
  }));
  expect(visualDiagnostics.failedScripts).toEqual([
    { path: '/static/thread-watcher.v1.js', error: 'net::ERR_CONNECTION_FAILED' },
  ]);
  const serialized = JSON.stringify(visualDiagnostics.failedScripts);
  expect(serialized).not.toContain('QUERY_SENTINEL');
  expect(serialized).not.toContain('FRAGMENT_SENTINEL');
  await expect(page.locator('.postMenuBtn').first()).toBeVisible();
});

test('synthetic visual state excludes form, cookie and storage contents', async ({ page, context }) => {
  await context.addCookies([{ name: 'owned-private', value: 'COOKIE_SENTINEL', url: 'http://127.0.0.1:3000' }]);
  await page.goto('/demo/');
  await page.evaluate(() => {
    document.querySelector('#password').value = 'PASSWORD_SENTINEL';
    document.querySelector('#com').value = 'COMMENT_SENTINEL';
    localStorage.setItem('owned-private', 'STORAGE_SENTINEL');
    history.replaceState(null, '', '?private=QUERY_SENTINEL#FRAGMENT_SENTINEL');
  });
  await page.locator('.postInfo > .postNum').first().click();
  await expect(page.locator('#quickReply')).toBeVisible();
  const state = await readVisualState(page), serialized = JSON.stringify(state);
  expect(state.quickReply).toBe(true); expect(state.nativeForm).toBe(true);
  expect(state.events.at(-1)).toMatchObject({ type: 'click', quote: true, prevented: true, quickReply: true });
  for (const privateValue of ['COOKIE_SENTINEL', 'PASSWORD_SENTINEL', 'COMMENT_SENTINEL', 'STORAGE_SENTINEL', 'QUERY_SENTINEL', 'FRAGMENT_SENTINEL']) {
    expect(serialized).not.toContain(privateValue);
  }
});
