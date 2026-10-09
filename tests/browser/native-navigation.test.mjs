import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { admitNavigationGroups, navigationPage, parseNavigationDirectory, navigationDirectory } from '../../apps/public/static/native-navigation.v1.js';

const origin = 'https://navigation.example';
const directory = { version: 1, boards: [{ board: 'demo', title: 'Owned <img src=x> title' }, { board: 'test', title: 'Owned Test' }] };

test('navigation uses canonical local pages and a finite exact board schema', () => {
  assert.equal(navigationPage('/test/', 'test'), 0);
  assert.equal(navigationPage('/test/0', 'test'), 0);
  assert.equal(navigationPage('/test/999', 'test'), 999);
  for (const path of ['/test/00', '/test/01', '/test/1000', '/demo/1', '//evil/test/1', '/test/1?q=x', '/test/thread/1']) assert.equal(navigationPage(path, 'test'), null);
  assert.deepEqual(parseNavigationDirectory(JSON.stringify(directory)), directory.boards);
  for (const value of [null, { ...directory, extra: true }, { ...directory, version: 2 }, { ...directory, boards: Array(101).fill(directory.boards[0]) },
    { ...directory, boards: [directory.boards[0], directory.boards[0]] },
    { ...directory, boards: [{ board: '../test', title: 'test' }] },
    { ...directory, boards: [{ board: 'test', title: 'x'.repeat(121) }] },
    { ...directory, boards: [{ board: 'test', title: 'test', url: 'https://evil.test' }] }]) {
    assert.throws(() => parseNavigationDirectory(JSON.stringify(value)));
  }
  assert.throws(() => parseNavigationDirectory(' '.repeat(32769)));
});

test('desktop group admission preserves immutable order, exact labels and local view destinations', () => {
  const entry = (board, title, href = `/${board}/`) => ({ board, title, href, nws: false });
  const input = [[entry('v', 'Video Games', '/v/archive'), entry('a', 'Anime & Manga', '/a/catalog')],
    [{ ...entry('b', 'Random'), nws: true }, entry('f', 'Flash')]];
  assert.deepEqual(admitNavigationGroups([]), []);
  assert.ok(Object.isFrozen(admitNavigationGroups([])));
  assert.deepEqual(admitNavigationGroups([[entry('a', 'Anime & Manga')], [entry('i', 'Oekaki')]]).map(group => group.map(item => item.board)), [['a'], ['i']]);
  const admitted = admitNavigationGroups(input);
  assert.deepEqual(admitted, input);
  input[0][0].title = 'Changed'; input[0].reverse(); input.push([entry('extra', 'Extra')]);
  assert.equal(admitted[0][0].title, 'Video Games');
  assert.equal(admitted.length, 2);
  assert.ok(Object.isFrozen(admitted) && Object.isFrozen(admitted[0]) && Object.isFrozen(admitted[0][0]));
  for (const href of ['//evil.test/a/', 'https://evil.test/a/', '/v/', '/a/../v/', '/a/?x=1', '/a/thread/1', 'javascript:alert(1)']) {
    assert.throws(() => admitNavigationGroups([[entry('a', 'Anime & Manga', href)]]));
  }
  for (const value of [[[]], Array(6).fill([entry('a', 'A')]), [[entry('a', 'A'), entry('a', 'A')]],
    [[entry('f', 'Flash', '/f/catalog')]], [[entry('b', 'Random', '/b/archive')]],
    [[{ ...entry('a', 'A'), onclick: 'evil' }]], [[entry('a', 'x'.repeat(121))]],
    [Array.from({ length: 78 }, (_, i) => entry(`b${i}`, 'Board'))]]) assert.throws(() => admitNavigationGroups(value));
  assert.equal(admitNavigationGroups([[entry('a', '<img src=x>')]])[0][0].title, '<img src=x>');
});

test('directory transport bounds authority, deadlines and empty-chunk stream work', async () => {
  const response = (body, options = {}) => ({ url: `${origin}/_watch/boards`, status: 200, redirected: false,
    headers: new Headers({ 'content-type': 'application/json' }), body: new Response(body).body, ...options });
  const healthy = async (url, options) => {
    assert.equal(url, `${origin}/_watch/boards`);
    assert.equal(options.credentials, 'omit'); assert.equal(options.redirect, 'error'); assert.equal(options.mode, 'same-origin');
    return response(JSON.stringify(directory));
  };
  assert.deepEqual(await navigationDirectory({ origin, fetcher: healthy }), { status: 'ok', boards: directory.boards });
  for (const value of [response('{}'), response(JSON.stringify(directory), { url: `${origin}/redirect` }),
    response(JSON.stringify(directory), { redirected: true }), response(JSON.stringify(directory), { status: 404 }),
    response(JSON.stringify(directory), { headers: new Headers({ 'content-type': 'text/html' }) }),
    response(JSON.stringify(directory), { headers: new Headers({ 'content-type': 'application/json', 'content-length': '32769' }) })]) {
    assert.notEqual((await navigationDirectory({ origin, fetcher: async () => value })).status, 'ok');
  }
  assert.equal((await navigationDirectory({ origin, fetcher: () => new Promise(() => {}), requestMs: 10 })).status, 'timeout');
  let reads = 0, cancelled = 0;
  const stream = response('', { body: { getReader: () => ({
    async read() { reads++; return { done: false, value: new Uint8Array() }; }, async cancel() { cancelled++; },
  }) } });
  assert.equal((await navigationDirectory({ origin, fetcher: async () => stream })).status, 'response-limit');
  assert.equal(reads, 4097); assert.equal(cancelled, 1);
  const controller = new AbortController();
  const pending = navigationDirectory({ origin, signal: controller.signal, fetcher: () => new Promise(() => {}) });
  controller.abort(); assert.equal((await pending).status, 'cancelled');
  assert.equal((await navigationDirectory({ origin, fetcher: healthy })).status, 'ok');
});

test('navigation controls own only their layout, local links and finite saved positions', async t => {
  const files = {};
  for (const name of ['native-navigation.v1.js', 'native-display.v1.js', 'watcher-position.v1.js', 'page-chrome.v1.js']) {
    files[`/static/${name}`] = await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8');
  }
  const css = await readFile(new URL('../../apps/public/static/board.css', import.meta.url), 'utf8');
  const fade = await readFile(new URL('../../apps/public/static/themes/fade.png', import.meta.url));
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup(config = {}, catalog = false, server = false) {
      const context = await browser.newContext({ viewport: { width: 1000, height: 700 } });
      const page = await context.newPage(), requests = [], errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.route('**/*', async route => {
        const url = new URL(route.request().url());
        if (url.origin === origin && url.pathname === '/static/themes/fade.png') return route.fulfill({ contentType: 'image/png', body: fade });
        if (files[url.pathname]) return route.fulfill({ contentType: 'text/javascript', body: files[url.pathname] });
        if (url.pathname === '/_watch/boards') { requests.push(url.pathname); return route.fulfill({ contentType: 'application/json', body: JSON.stringify(directory) }); }
        if (url.pathname === (catalog ? '/test/catalog' : '/test/1')) return route.fulfill({ contentType: 'text/html', body:
          `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><style>${css}</style></head><body${server ? ' class="publicPageChrome"' : ''}><nav class="boardList">[ <a href="/">All boards</a> ]</nav>${server ? '<div id="boardNavDesktop"><span data-public-board-list><span data-public-board-group>[<a href="/test/archive" title="Desktop Test">test</a>]</span> <span data-public-board-group>[<span class="nwsb"><a href="/demo/archive" title="Demo &lt;b&gt;board&lt;/b&gt;">demo</a></span>]</span></span></div><select id="boardSelectMobile"><option value="demo" class="nwsb">/demo/ - Demo &lt;b&gt;board&lt;/b&gt;</option><option value="test" selected>/test/ - Owned Test</option></select>' : ''}<main><textarea id="draft">Owned draft</textarea><div style="height:3000px">Owned scroll area</div><nav class="pages"><a href="/test/0" rel="prev">Previous</a><a href="/test/2" rel="next">Next</a></nav></main></body></html>` });
        if (url.pathname.endsWith('favicon.ico')) return route.fulfill({ status: 204 });
        requests.push(url.href); return route.abort();
      });
      await page.goto(`${origin}/test/${catalog ? 'catalog' : '1'}`);
      await page.evaluate(async ({ config, catalog }) => {
        window.config = config; window.saved = []; window.heldSaves = []; window.holdSave = false;
        window.original = document.querySelector('nav.boardList'); window.draft = document.querySelector('#draft');
        window.settingsOpened = window.editorOpened = 0;
        const { mountNativeNavigation } = await import('/static/native-navigation.v1.js');
        window.mountNavigation = () => mountNativeNavigation({
          root: document.body, board: 'test', thread: null, catalog, settings: () => config,
          mobile: matchMedia('(max-width:480px)'), readNeverMobile: () => config.neverMobile,
          openSettings: () => settingsOpened++, openCustomMenu: () => editorOpened++,
          savePosition: async (key, value, expected, signal) => {
            if (holdSave) await new Promise(resolve => heldSaves.push({ resolve, signal }));
            if (signal.aborted || (config[key] ?? null) !== expected) return false;
            config[key] = value; saved.push({ key, value }); return true;
          },
        });
        window.navigation = mountNavigation();
      }, { config, catalog });
      return { context, page, requests, errors };
    }

    await t.test('default-off controls preserve original nodes and make no directory request', async () => {
      const { context, page, requests, errors } = await setup();
      try {
        assert.equal(await page.locator('.nativePersistentNavigation,.topPageNav,#stickyNav').count(), 0);
        assert.deepEqual(requests, []); assert.deepEqual(errors, []);
        assert.equal(await page.evaluate(() => original.isConnected && draft.value === 'Owned draft'), true);
      } finally { await context.close(); }
    });

    await t.test('fixed and classic menus use escaped local links and restore after disabling', async () => {
      const { context, page, requests, errors } = await setup({ dropDownNav: true });
      try {
        await page.waitForFunction(() => document.querySelectorAll('.nativePersistentNavigation option').length === 2);
        assert.equal(await page.locator('.nativePersistentNavigation img').count(), 0);
        assert.equal(await page.getByLabel('Board', { exact: true }).inputValue(), 'test');
        await page.getByRole('button', { name: 'Settings', exact: true }).click();
        await page.getByRole('button', { name: 'Edit boards', exact: true }).click();
        assert.deepEqual(await page.evaluate(() => [settingsOpened, editorOpened]), [1, 1]);
        await page.evaluate(() => { config.classicNav = true; config.customMenu = true; config.customMenuList = 'test demo test'; navigation.refresh(); });
        assert.deepEqual(await page.locator('.nativeBoardLinks a').evaluateAll(nodes => nodes.map(a => a.getAttribute('href'))), ['/test/', '/demo/', '/test/']);
        await page.evaluate(() => { config.customMenuList = Array(64).fill('longboard1').join(' '); navigation.refresh(); });
        await page.waitForFunction(() => {
          const bar = document.querySelector('.nativePersistentNavigation').getBoundingClientRect();
          return document.querySelector('#draft').getBoundingClientRect().top >= bar.bottom;
        });
        assert.ok((await page.locator('.nativePersistentNavigation').boundingBox()).height <= 350);
        await page.setViewportSize({ width: 390, height: 700 });
        await page.waitForFunction(() => !document.querySelector('.nativePersistentNavigation'));
        assert.equal(await page.locator('.nativeBoardLinks').count(), 0);
        assert.equal(await page.evaluate(() => config.dropDownNav), true);
        await page.evaluate(() => { config.neverMobile = 'true'; navigation.refresh(); });
        await page.waitForFunction(() => document.querySelector('.nativePersistentNavigation .nativeBoardLinks'));
        assert.equal(await page.evaluate(() => config.dropDownNav), true);
        await page.evaluate(() => { config.disableAll = true; navigation.refresh(); });
        assert.equal(await page.locator('.nativePersistentNavigation').count(), 0);
        assert.equal(await page.evaluate(() => original.isConnected && !document.body.classList.contains('hasDropDownNav') && draft.value === 'Owned draft'), true);
        assert.equal(await page.evaluate(() => document.body.style.getPropertyValue('--native-navigation-height')), '');
        assert.deepEqual(requests, ['/_watch/boards']); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('a catalog custom menu keeps the full drop-down directory and separate index links', async () => {
      const { context, page, requests, errors } = await setup({ dropDownNav: true, customMenu: true, customMenuList: 'test' }, true);
      try {
        await page.waitForFunction(() => [...document.querySelectorAll('.nativePersistentNavigation option')].some(option => option.textContent.includes('Owned Test')));
        assert.deepEqual(await page.locator('.nativePersistentNavigation option').evaluateAll(nodes => nodes.map(node => node.value)), ['demo', 'test']);
        assert.deepEqual(await page.locator('.nativeCustomBoardLinks a').evaluateAll(nodes => nodes.map(node => node.getAttribute('href'))), ['/test/']);
        assert.equal(await page.getByLabel('Board', { exact: true }).inputValue(), 'test');
        assert.deepEqual(requests, ['/_watch/boards']); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('the bounded server directory retains safety classes and needs no fetch', async () => {
      const { context, page, requests, errors } = await setup({ dropDownNav: true, customMenu: true, customMenuList: 'test' }, true, true);
      try {
        assert.deepEqual(await page.locator('.nativePersistentNavigation option').evaluateAll(options => options.map(option => ({ value: option.value, label: option.textContent, class: option.className }))),
          [{ value: 'demo', label: '/demo/ - Demo <b>board</b>', class: 'nwsb' }, { value: 'test', label: '/test/ - Owned Test', class: '' }]);
        assert.equal(await page.locator('.nativePersistentNavigation b').count(), 0);
        await page.evaluate(() => {
          const injected = document.createElement('option'); injected.value = 'later'; injected.textContent = '/later/ - Injected after mount';
          document.getElementById('boardSelectMobile').append(injected); config.customMenuList = 'demo'; navigation.refresh();
        });
        assert.deepEqual(await page.locator('.nativePersistentNavigation option').evaluateAll(options => options.map(option => option.value)), ['demo', 'test']);
        assert.deepEqual(await page.locator('.nativeCustomBoardLinks a').evaluateAll(links => links.map(link => link.getAttribute('href'))), ['/demo/']);
        assert.deepEqual(requests, []); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('classic snapshots retain desktop groups independently of mobile and restore after custom menus', async () => {
      const { context, page, requests, errors } = await setup({ dropDownNav: true, classicNav: true }, false, true);
      try {
        const groups = () => page.locator('.nativeBoardLinks [data-public-board-group]').evaluateAll(groups =>
          groups.map(group => [...group.querySelectorAll('a')].map(a => [a.textContent, a.title, a.getAttribute('href')])));
        const expected = [[['test', 'Desktop Test', '/test/archive']], [['demo', 'Demo <b>board</b>', '/demo/archive']]];
        assert.deepEqual(await groups(), expected);
        assert.equal(await page.locator('.nativeBoardLinks').textContent(), '[test] [demo]');
        assert.equal(await page.locator('.nativeBoardLinks .nwsb a').textContent(), 'demo');
        assert.equal(await page.locator('.nativeBoardLinks b').count(), 0);
        await page.evaluate(() => {
          document.querySelector('#boardNavDesktop a').setAttribute('href', '//evil.example/');
          config.customMenu = true; config.customMenuList = 'demo test'; navigation.refresh();
        });
        assert.deepEqual(await page.locator('.nativeBoardLinks a').evaluateAll(links => links.map(a => a.getAttribute('href'))), ['/demo/', '/test/']);
        await page.evaluate(() => { config.customMenu = false; navigation.refresh(); });
        assert.deepEqual(await groups(), expected);
        await page.evaluate(() => { config.classicNav = false; navigation.refresh(); });
        assert.deepEqual(await page.locator('.nativePersistentNavigation option').evaluateAll(options => options.map(o => o.textContent)),
          ['/demo/ - Demo <b>board</b>', '/test/ - Owned Test']);
        await page.evaluate(() => {
          config.classicNav = true; navigation.refresh();
          dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
          dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
          dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
        });
        assert.deepEqual(await groups(), expected);
        assert.equal(await page.locator('.nativePersistentNavigation').count(), 1);
        assert.deepEqual(requests, []); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('empty or rejected public groups never widen to the full discovery directory', async () => {
      const { context, page, requests, errors } = await setup({ dropDownNav: true }, false, true);
      try {
        await page.evaluate(() => {
          document.querySelector('#boardNavDesktop [data-public-board-list]').replaceChildren();
          document.querySelector('#boardSelectMobile').options[0].textContent = 'Invalid label';
          navigation = mountNavigation();
        });
        assert.deepEqual(await page.locator('.nativePersistentNavigation option').evaluateAll(options => options.map(o => o.value)), ['test']);
        assert.equal(await page.locator('.nativePersistentNavigation option[data-current-board-fallback]').count(), 1);
        await page.evaluate(() => { config.classicNav = true; navigation.refresh(); });
        assert.equal(await page.locator('.nativeBoardLinks a').count(), 0);
        assert.deepEqual(requests, []); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('an invalid nonempty desktop group fails closed without poisoning valid mobile choices', async () => {
      const { context, page, requests, errors } = await setup({ dropDownNav: true, classicNav: true }, false, true);
      try {
        await page.evaluate(() => {
          const list = document.querySelector('#boardNavDesktop [data-public-board-list]');
          list.lastElementChild.remove();
          list.querySelector('a').setAttribute('href', '//evil.example/');
          navigation = mountNavigation();
        });
        assert.equal(await page.locator('#boardNavDesktop [data-public-board-group]').count(), 1);
        assert.equal(await page.locator('.nativeBoardLinks a').count(), 0);
        await page.evaluate(() => { config.classicNav = false; navigation.refresh(); });
        assert.deepEqual(await page.locator('.nativePersistentNavigation option').evaluateAll(options => options.map(o => o.value)), ['demo', 'test']);
        assert.equal(await page.getByLabel('Board', { exact: true }).inputValue(), 'test');
        assert.deepEqual(requests, []); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('removed and suspended drop-down selectors cannot navigate through stale events', async () => {
      for (const suspended of [false, true]) {
        const { context, page, requests, errors } = await setup({ dropDownNav: true });
        try {
          await page.waitForFunction(() => document.querySelectorAll('.nativePersistentNavigation option').length === 2);
          await page.evaluate(suspended => {
            const stale = document.querySelector('.nativePersistentNavigation select');
            if (suspended) dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
            else { config.dropDownNav = false; navigation.refresh(); }
            stale.value = 'demo'; stale.dispatchEvent(new Event('change'));
          }, suspended);
          await page.waitForLoadState('networkidle');
          assert.equal(page.url(), `${origin}/test/1`);
          assert.deepEqual(requests, ['/_watch/boards']); assert.deepEqual(errors, []);
        } finally { await context.close(); }
      }
    });

    await t.test('auto-hide follows scroll direction, respects focus and cannot outlive its setting', async () => {
      const { context, page, errors } = await setup({ dropDownNav: true, autoHideNav: true });
      try {
        await page.evaluate(() => scrollTo(0, 700));
        await page.waitForFunction(() => document.querySelector('.nativePersistentNavigation').getBoundingClientRect().bottom <= 0);
        await page.evaluate(() => scrollTo(0, 400));
        await page.waitForFunction(() => document.querySelector('.nativePersistentNavigation').getBoundingClientRect().top === 0);
        await page.getByRole('button', { name: 'Settings', exact: true }).focus();
        await page.evaluate(() => scrollTo(0, 900));
        await new Promise(resolve => setTimeout(resolve, 80));
        assert.equal(await page.locator('.nativePersistentNavigation').evaluate(node => node.style.top), '');
        await page.evaluate(() => { document.activeElement.blur(); scrollTo(0, 800); });
        await new Promise(resolve => setTimeout(resolve, 80));
        assert.equal(await page.locator('.nativePersistentNavigation').evaluate(node => node.style.top), '');
        await page.evaluate(() => { config.autoHideNav = false; navigation.refresh(); scrollTo(0, 1000); });
        assert.equal(await page.locator('.nativePersistentNavigation').evaluate(node => node.style.top), '');
        await page.evaluate(() => { config.autoHideNav = true; navigation.refresh(); scrollTo(0, 900); });
        await new Promise(resolve => setTimeout(resolve, 80));
        assert.equal(await page.locator('.nativePersistentNavigation').evaluate(node => node.style.top), '');
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('Shift-key movement is bounded, saved, and cancelled on disable before a delayed write', async () => {
      const { context, page, errors } = await setup({ topPageNav: true, stickyNav: true, 'TN-position': 'left: 10px;', 'SN-position': 'right: 9999%; top: 9999%;' });
      try {
        const box = await page.locator('#stickyNav').boundingBox();
        assert.ok(box.x >= 0 && box.y >= 0 && box.x + box.width <= 1000 && box.y + box.height <= 700);
        assert.deepEqual(await page.locator('.topPageNav a').evaluateAll(nodes => nodes.map(a => a.getAttribute('href'))), ['/test/0', '/test/2']);
        const handle = page.locator('.topPageNav > div');
        const before = await handle.boundingBox(); await handle.focus(); await page.keyboard.press('Shift+ArrowRight');
        await page.waitForFunction(() => saved.length === 1);
        assert.ok((await handle.boundingBox()).x > before.x);
        await page.evaluate(() => { holdSave = true; });
        await page.keyboard.press('Shift+ArrowRight');
        await page.waitForFunction(() => heldSaves.length === 1);
        await page.evaluate(() => { config['TN-position'] = 'left: 77px; top: 88px;'; navigation.refresh(); });
        assert.equal(await page.evaluate(() => heldSaves[0].signal.aborted), true);
        await page.evaluate(async () => { heldSaves[0].resolve(); await Promise.resolve(); });
        await page.waitForFunction(() => document.querySelector('.topPageNav').style.left === '77px');
        assert.equal(await page.evaluate(() => config['TN-position']), 'left: 77px; top: 88px;');
        assert.equal(await page.evaluate(() => saved.length), 1);
        await page.keyboard.press('Shift+ArrowRight');
        await page.waitForFunction(() => heldSaves.length === 2);
        await page.evaluate(() => { config.disableAll = true; navigation.refresh(); });
        assert.equal(await page.evaluate(() => heldSaves[1].signal.aborted), true);
        await page.evaluate(async () => { heldSaves[1].resolve(); await Promise.resolve(); });
        assert.equal(await page.evaluate(() => saved.length), 1);
        assert.equal(await page.locator('.topPageNav,#stickyNav').count(), 0);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('persistent bar bounds movable controls and repeated mounting replaces the old owner', async () => {
      const { context, page, errors } = await setup({ dropDownNav: true, topPageNav: true, stickyNav: true,
        'TN-position': 'left: 10px; top: 0px;', 'SN-position': 'left: 10px; top: 0px;' });
      try {
        const geometry = await page.evaluate(() => {
          const bar = document.querySelector('.nativePersistentNavigation').getBoundingClientRect();
          const top = document.querySelector('.topPageNav').getBoundingClientRect();
          const sticky = document.querySelector('#stickyNav').getBoundingClientRect();
          return { barBottom: bar.bottom, top: top.top, sticky: sticky.top };
        });
        assert.ok(geometry.top >= geometry.barBottom - 1);
        assert.ok(geometry.sticky >= geometry.barBottom - 1);

        await page.evaluate(() => { config.autoHideNav = true; navigation.refresh(); });
        assert.equal(await page.locator('.topPageNav').evaluate(node => node.style.top), '0px');
        assert.equal(await page.locator('#stickyNav').evaluate(node => node.style.top), '0px');
        await page.evaluate(() => { config.autoHideNav = false; navigation.refresh(); });
        assert.ok((await page.locator('.topPageNav').boundingBox()).y >= (await page.locator('.nativePersistentNavigation').boundingBox()).height - 1);

        await page.evaluate(() => { window.firstNavigation = navigation; navigation = mountNavigation(); });
        assert.equal(await page.locator('.nativePersistentNavigation,.topPageNav,#stickyNav').count(), 3);
        await page.evaluate(() => firstNavigation.destroy());
        assert.equal(await page.locator('.nativePersistentNavigation,.topPageNav,#stickyNav').count(), 3);
        assert.equal(await page.evaluate(() => document.body.classList.contains('hasDropDownNav')), true);
        assert.notEqual(await page.evaluate(() => document.body.style.getPropertyValue('--native-navigation-height')), '');
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('history restoration produces one control set and final teardown keeps original navigation', async () => {
      const { context, page, errors } = await setup({ topPageNav: true, stickyNav: true, dropDownNav: true });
      try {
        await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
        assert.equal(await page.locator('.nativePersistentNavigation,.topPageNav,#stickyNav').count(), 0);
        await page.evaluate(() => {
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
        });
        assert.equal(await page.locator('.nativePersistentNavigation,.topPageNav,#stickyNav').count(), 3);
        await page.evaluate(() => { navigation.destroy(); document.dispatchEvent(new Event('4chanSettingsSaved')); });
        assert.equal(await page.locator('.nativePersistentNavigation,.topPageNav,#stickyNav').count(), 0);
        assert.equal(await page.evaluate(() => original.isConnected && draft === document.querySelector('#draft')), true);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });
  } finally { await browser.close(); }
});
