import { test, expect, readVisualState } from '../helpers/visual-diagnostics.js';
test.use({ javaScriptEnabled: true });

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
