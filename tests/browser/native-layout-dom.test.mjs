import test, { after, before } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

const source = await readFile(new URL('../../apps/public/static/native-layout.v1.js', import.meta.url), 'utf8');
const imageSource = await readFile(new URL('../../apps/public/client/native-images.js', import.meta.url), 'utf8');
const origin = 'https://layout.example';
let browser;

before(async () => { browser = await chromium.launch({ headless: true }); });
after(async () => { await browser?.close(); });

async function fixture(t, settings, { initialLayout = null, href = '/static/theme.css?worksafe=true' } = {}) {
  const context = await browser.newContext({ viewport: { width: 1024, height: 700 } });
  t.after(() => context.close());
  await context.addInitScript(value => {
    localStorage.setItem('4chan-settings', JSON.stringify(value));
  }, settings);
  await context.route('**/*', route => {
    const url = new URL(route.request().url());
    if (route.request().isNavigationRequest()) {
      return route.fulfill({ contentType: 'text/html', body: `<!doctype html><html><head>
        <link id="theme-sheet" rel="stylesheet" href="${href}">
        </head><body><main class="board"><section class="thread"><article class="postContainer"><div class="sideArrows"></div></article><span class="summary"></span></section></main></body></html>` });
    }
    if (url.origin === origin && url.pathname === '/static/theme.css') {
      const family = url.searchParams.get('theme') === 'tomorrow' ? 'tomorrow'
        : url.searchParams.get('worksafe') === 'false' ? 'burichan'
          : url.searchParams.get('worksafe') === 'true' ? 'photon' : null;
      return route.fulfill({ contentType: 'text/css', body: `:root { --fixture-theme: loaded;${family ? ` --watcher-icon-family: ${family};` : ''} }` });
    }
    return route.abort();
  });
  const page = await context.newPage();
  await page.goto(`${origin}/demo/thread/123`);
  await page.evaluate(async ({ source, initialLayout }) => {
    if (initialLayout !== null) document.body.setAttribute('data-native-thread-layout', initialLayout);
    const moduleUrl = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
    const api = await import(moduleUrl);
    URL.revokeObjectURL(moduleUrl);
    window.layoutApi = api;
    window.themeChanges = [];
    document.addEventListener(api.THEME_READY_EVENT, () => {
      const value = getComputedStyle(document.documentElement).getPropertyValue('--watcher-icon-family').trim();
      window.themeChanges.push(['futaba', 'burichan', 'tomorrow', 'photon'].includes(value) ? value : 'futaba');
    });
    window.layoutController = api.mountNativeLayout({
      root: document.body,
      settings: () => JSON.parse(localStorage.getItem('4chan-settings') || '{}'),
      mobile: matchMedia('(max-width: 480px)'),
      readNeverMobile: () => localStorage.getItem('4chan_never_show_mobile'),
      themeStylesheet: document.getElementById('theme-sheet'),
    });
  }, { source, initialLayout });
  return { context, page };
}

const state = page => page.evaluate(() => ({
  layout: document.body.getAttribute('data-native-thread-layout'),
  href: document.getElementById('theme-sheet').getAttribute('href'),
  family: getComputedStyle(document.documentElement).getPropertyValue('--watcher-icon-family').trim(),
  themeChanges: [...window.themeChanges],
}));

test('settings, viewport and exact never-mobile state preserve source layout precedence', async t => {
  const { context, page } = await fixture(t, { compactThreads: true, centeredThreads: true });
  assert.deepEqual(await state(page), {
    layout: 'compact', href: '/static/theme.css?worksafe=true', family: 'photon', themeChanges: [],
  });

  await page.setViewportSize({ width: 390, height: 700 });
  await page.waitForFunction(() => document.body.dataset.nativeThreadLayout === 'centered');
  assert.equal((await state(page)).layout, 'centered');

  await page.evaluate(() => {
    localStorage.setItem('4chan_never_show_mobile', 'true');
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile' }));
  });
  assert.equal((await state(page)).layout, 'compact');

  await page.evaluate(() => {
    localStorage.removeItem('4chan_never_show_mobile');
    localStorage.setItem('4chan-settings', JSON.stringify({ centeredThreads: true }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.equal((await state(page)).layout, 'centered');

  const other = await context.newPage();
  await other.goto(`${origin}/demo/thread/123`);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({
    compactThreads: true, centeredThreads: true, disableAll: true,
  })));
  await page.waitForFunction(() => !document.body.hasAttribute('data-native-thread-layout'));
  assert.equal((await state(page)).layout, null);
  await other.close();
});

test('dark theme overrides only the local theme link and restores the latest requested href', async t => {
  const { page } = await fixture(t, { darkTheme: true });
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=true&theme=tomorrow');
  await page.waitForFunction(() => themeChanges.at(-1) === 'tomorrow');
  assert.equal((await state(page)).family, 'tomorrow');

  await page.evaluate(() => document.getElementById('theme-sheet').dispatchEvent(new Event('error')));
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=true');
  await page.waitForFunction(() => themeChanges.at(-1) === 'photon');
  await page.evaluate(() => document.dispatchEvent(new CustomEvent('4chanSettingsSaved')));
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=true');

  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ darkTheme: false }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
    localStorage.setItem('4chan-settings', JSON.stringify({ darkTheme: true }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=true&theme=tomorrow');
  await page.waitForFunction(() => themeChanges.at(-1) === 'tomorrow');

  await page.evaluate(() => {
    document.getElementById('theme-sheet').setAttribute('href', '/static/theme.css?worksafe=false');
  });
  await page.waitForFunction(() => getComputedStyle(document.documentElement).getPropertyValue('--watcher-icon-family').trim() === 'burichan');
  await page.evaluate(() => document.dispatchEvent(new CustomEvent('4chanSettingsSaved')));
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=false&theme=tomorrow');
  await page.waitForFunction(() => themeChanges.at(-1) === 'tomorrow');

  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ darkTheme: false }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
    layoutController.refresh();
  });
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=false');
  await page.waitForFunction(() => themeChanges.at(-1) === 'burichan');

  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ darkTheme: true, disableAll: true }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=false');
});

test('mobile dark classes and exact never-mobile state preserve the selected desktop stylesheet', async t => {
  const { page } = await fixture(t, { darkTheme: true });
  await page.waitForFunction(() => themeChanges.at(-1) === 'tomorrow');
  await page.setViewportSize({ width: 390, height: 700 });
  await page.waitForFunction(() => document.body.classList.contains('m-dark'));
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=true');
  assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), 'false');
  await page.evaluate(() => {
    localStorage.setItem('4chan_never_show_mobile', 'true');
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile' }));
  });
  assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), 'true');
  assert.equal(await page.locator('body').evaluate(node => node.classList.contains('m-dark')), false);
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=true&theme=tomorrow');
  await page.evaluate(() => {
    localStorage.setItem('4chan_never_show_mobile', 'TRUE');
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile' }));
  });
  assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), 'false');
  assert.equal(await page.locator('body').evaluate(node => node.classList.contains('m-dark')), true);
  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ darkTheme: true, disableAll: true }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.equal(await page.locator('body').evaluate(node => node.classList.contains('m-dark')), false);
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=true');
  await page.evaluate(() => layoutController.destroy());
  assert.equal(await page.locator('body').getAttribute('data-native-never-mobile'), null);
});

test('BFCache recomputes on persisted restore and teardown restores only owned state', async t => {
  const { page } = await fixture(t, { compactThreads: true, darkTheme: true }, { initialLayout: 'host-state' });
  await page.waitForFunction(() => themeChanges.at(-1) === 'tomorrow');
  assert.deepEqual(await state(page), {
    layout: 'compact', href: '/static/theme.css?worksafe=true&theme=tomorrow', family: 'tomorrow', themeChanges: ['tomorrow'],
  });

  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ centeredThreads: true, darkTheme: false }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.deepEqual(await state(page), {
    layout: 'compact', href: '/static/theme.css?worksafe=true&theme=tomorrow', family: 'tomorrow', themeChanges: ['tomorrow'],
  });

  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  await page.waitForFunction(() => themeChanges.at(-1) === 'photon');
  assert.deepEqual(await state(page), {
    layout: 'centered', href: '/static/theme.css?worksafe=true', family: 'photon', themeChanges: ['tomorrow', 'photon'],
  });

  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ compactThreads: true, darkTheme: true }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
    // Teardown must run in the same browser task to cancel the pending theme
    // notification. Separate evaluate calls permit it to settle between them.
    window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: false }));
  });
  assert.equal((await state(page)).layout, 'host-state');
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=true');
  assert.deepEqual((await state(page)).themeChanges, ['tomorrow', 'photon']);

  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ centeredThreads: true, darkTheme: true }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.equal((await state(page)).layout, 'host-state');
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=true');
  assert.deepEqual((await state(page)).themeChanges, ['tomorrow', 'photon']);
});

test('teardown does not overwrite a theme href or layout marker replaced by another owner', async t => {
  const { page } = await fixture(t, { compactThreads: true, darkTheme: true });
  await page.waitForFunction(() => themeChanges.at(-1) === 'tomorrow');
  await page.evaluate(() => {
    document.body.setAttribute('data-native-thread-layout', 'external');
    document.getElementById('theme-sheet').setAttribute('href', '/static/theme.css?worksafe=false');
    layoutController.destroy();
  });
  assert.equal((await state(page)).layout, 'external');
  assert.equal((await state(page)).href, '/static/theme.css?worksafe=false');
  assert.deepEqual((await state(page)).themeChanges, ['tomorrow']);
});

test('theme-ready requires the expected loaded family and native images refresh their finite family dynamically', async t => {
  const { page } = await fixture(t, { darkTheme: false });
  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ darkTheme: true }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
    document.getElementById('theme-sheet').dispatchEvent(new Event('load'));
    window.earlyThemeChanges = [...themeChanges];
  });
  assert.deepEqual(await page.evaluate(() => earlyThemeChanges), []);
  await page.waitForFunction(() => themeChanges.at(-1) === 'tomorrow');

  await page.evaluate(async imageSource => {
    const moduleUrl = URL.createObjectURL(new Blob([imageSource], { type: 'text/javascript' }));
    const images = await import(moduleUrl);
    URL.revokeObjectURL(moduleUrl);
    window.imageFamily = 'tomorrow';
    window.imageController = images.mountNativeImages({
      root: document.querySelector('.board'), mediaOrigin: 'https://media.example', settings: () => ({ noPictures: true }),
      mobile: matchMedia('(max-width: 480px)'), family: () => imageFamily,
    });
  }, imageSource);
  assert.deepEqual(await page.locator('.board').evaluate(node => ({ family: node.dataset.imageFamily, noPictures: node.classList.contains('noPictures') })),
    { family: 'tomorrow', noPictures: true });
  await page.evaluate(() => { imageFamily = 'photon'; imageController.refresh(); });
  assert.equal(await page.locator('.board').getAttribute('data-image-family'), 'photon');
  await page.evaluate(() => { imageFamily = '<style>'; imageController.refresh(); });
  assert.equal(await page.locator('.board').getAttribute('data-image-family'), 'futaba');
  await page.evaluate(() => imageController.dispose());
});

test('ordinary Yotsuba restoration settles through the finite Futaba asset fallback', async t => {
  const { page } = await fixture(t, { darkTheme: true }, { href: '/static/theme.css' });
  await page.waitForFunction(() => themeChanges.at(-1) === 'tomorrow');
  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ darkTheme: false }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  await page.waitForFunction(() => themeChanges.at(-1) === 'futaba');
  assert.equal((await state(page)).href, '/static/theme.css');
  assert.equal((await state(page)).family, '');
  await page.evaluate(() => document.getElementById('theme-sheet').dispatchEvent(new Event('load')));
  assert.deepEqual((await state(page)).themeChanges, ['tomorrow', 'futaba']);
});
