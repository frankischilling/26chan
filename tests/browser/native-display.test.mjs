import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { customBoards, localeDate, DISPLAY_LIMITS } from '../../apps/public/static/native-display.v1.js';

test('board preferences are finite local names and never supply a URL or markup', () => {
  assert.deepEqual(customBoards('test demo TEST'), ['test', 'demo', 'test']);
  assert.deepEqual(customBoards(' /test/,,demo '), ['test', 'demo']);
  assert.deepEqual(customBoards(''), []);
  for (const value of [null, {}, [], 17, 'x'.repeat(11), Array(65).fill('x').join(' '), '😀'.repeat(257), ' '.repeat(1025)]) {
    assert.equal(customBoards(value), null);
  }
  for (const value of ['javascript:alert(1)', '<img src=x onerror=alert(1)>', '//evil.test/%2f..', 'a\0b\nC']) {
    assert.ok(customBoards(value).every(board => /^[a-z0-9]{1,10}$/.test(board)));
  }
  assert.equal(customBoards(Array(64).fill('test').join(' ')).length, DISPLAY_LIMITS.boards);
});

test('local dates reject missing offsets, malformed values and unbounded preferences', () => {
  for (const value of [null, 1, '', '2026-09-28', '2026-09-28T01:00:00', '2026-99-01T00:00:00Z', 'x'.repeat(65)]) {
    assert.equal(localeDate(value), null);
  }
  assert.equal(localeDate('2026-09-28T00:00:00Z', new Date(NaN)), null);
});

test('display controls preserve original content, finite navigation and lifecycle ownership', async t => {
  const origin = 'https://display.example';
  const files = {};
  for (const name of ['native-display.v1.js', 'native-filter.v1.js', 'native-backlinks.v1.js']) {
    files[`/static/${name}`] = await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8');
  }
  const html = '<!doctype html><html><head><meta charset="utf-8"></head><body>'
    + '<nav class="boardList" aria-label="Board navigation">[ <a href="/">All boards</a> ]</nav>'
    + '<main class="board"><article class="postContainer opContainer" id="pc100"><div class="post op" id="p100">'
    + '<div class="postInfo" id="pi100"><span class="name">Anonymous</span> '
    + '<time datetime="2026-03-08T09:59:59Z">03/08/26(Sun)05:59:59</time>'
    + '<a class="postNum" href="/test/thread/100#p100">No.100</a></div>'
    + '<blockquote class="postMessage" id="m100">Owned original comment</blockquote></div></article></main></body></html>';
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup(timezoneId = 'America/Los_Angeles', config = {}) {
      const context = await browser.newContext({ timezoneId });
      const page = await context.newPage(), requests = [];
      await context.route('**/*', async route => {
        const url = new URL(route.request().url());
        if (url.origin === origin && files[url.pathname]) {
          await route.fulfill({ contentType: 'text/javascript', body: files[url.pathname] });
        } else if (url.href === `${origin}/test/`) {
          await route.fulfill({ contentType: 'text/html', body: html });
        } else if (url.pathname === '/favicon.ico') await route.fulfill({ status: 204 });
        else { requests.push(url.href); await route.abort(); }
      });
      await page.goto(`${origin}/test/`);
      await page.evaluate(async config => {
        window.api = await import('/static/native-display.v1.js');
        window.quotes = await import('/static/native-filter.v1.js');
        window.projection = (await import('/static/native-backlinks.v1.js')).createCommentProjection();
        window.config = config; window.saved = []; window.opens = 0;
        window.display = api.mountNativeDisplay({ root: document.body, settings: () => config,
          projection, openSettings: () => window.opens++, save: async changes => {
            Object.assign(config, changes); window.saved.push(changes); return { persisted: false };
          } });
      }, config);
      return { context, page, requests };
    }

    await t.test('local-time formatting follows DST and non-hour offsets', async () => {
      for (const [timezone, first, second, title] of [
        ['America/Los_Angeles', '03/08/26(Sun)01:59:59', '03/08/26(Sun)03:00:00', 'Timezone: UTC-7'],
        ['Asia/Kathmandu', '03/08/26(Sun)15:44:59', '03/08/26(Sun)15:45:00', 'Timezone: UTC+5:45'],
        ['UTC', '03/08/26(Sun)09:59:59', '03/08/26(Sun)10:00:00', 'Timezone: UTC'],
      ]) {
        const { page, context, requests } = await setup(timezone);
        try {
          const result = await page.evaluate(() => [
            api.localeDate('2026-03-08T09:59:59Z', new Date('2026-09-28T12:00:00Z')),
            api.localeDate('2026-03-08T10:00:00Z', new Date('2026-09-28T12:00:00Z')),
          ]);
          assert.deepEqual(result, [{ text: first, title }, { text: second, title }]);
          assert.equal(await page.locator('#pi100 time').textContent(), first);
          assert.deepEqual(requests, []);
        } finally { await context.close(); }
      }
    });

    await t.test('quote copies read server text and restore it when local time is disabled', async () => {
      const { context, page, requests } = await setup();
      try {
        const result = await page.evaluate(() => {
          const context = { origin: location.origin, board: 'test', thread: '100', mediaOrigin: '' };
          const tree = quotes.localQuoteTree(document.getElementById('pc100'), context, '100', projection);
          const findTime = value => typeof value !== 'string' && (value.tag === 'time' ? value : value.children.map(findTime).find(Boolean));
          const copy = quotes.prepareQuotePost(tree, context, '100').build(document);
          copy.id = 'owned-copy'; document.querySelector('.board').append(copy);
          return findTime(tree);
        });
        assert.deepEqual(result, { tag: 'time', attrs: { datetime: '2026-03-08T09:59:59Z' }, children: ['03/08/26(Sun)05:59:59'] });
        await page.waitForFunction(() => document.querySelector('#owned-copy time').textContent === '03/08/26(Sun)01:59:59');
        await page.evaluate(() => { config.localTime = false; display.refresh(); });
        assert.deepEqual(await page.locator('.postInfo time').allTextContents(), ['03/08/26(Sun)05:59:59', '03/08/26(Sun)05:59:59']);
        assert.equal(await page.locator('time[title]').count(), 0);
        assert.equal(await page.locator('#m100').textContent(), 'Owned original comment');
        assert.deepEqual(requests, []);
      } finally { await context.close(); }
    });

    await t.test('navigation preserves order, restores Show all and cannot create external authority', async () => {
      const { context, page, requests } = await setup('UTC', { customMenu: true, customMenuList: 'demo test demo' });
      try {
        assert.deepEqual(await page.locator('.customBoardList a').evaluateAll(links => links.slice(0, 3).map(a => a.getAttribute('href'))), ['/demo/', '/test/', '/demo/']);
        assert.equal(await page.locator('nav:not(.customBoardList)').getAttribute('hidden'), '');
        await page.getByRole('link', { name: 'Settings', exact: true }).click();
        assert.equal(await page.evaluate(() => opens), 1);
        await page.getByRole('link', { name: 'Show all boards' }).click();
        await page.evaluate(() => display.refresh());
        assert.equal(await page.locator('.customBoardList').count(), 0);
        assert.equal(await page.locator('nav.boardList').getAttribute('hidden'), null);
        await page.evaluate(() => { config.customMenuList = '<img src=x onerror=alert(1)>'; display.refresh(); });
        assert.equal(await page.locator('.customBoardList img, .customBoardList script').count(), 0);
        assert.ok(await page.locator('.customBoardList a').evaluateAll(links => links.every(a => a.origin === location.origin)));
        await page.evaluate(() => { config.disableAll = true; display.refresh(); });
        assert.equal(await page.locator('.customBoardList').count(), 0);
        assert.deepEqual(requests, []);
      } finally { await context.close(); }
    });

    await t.test('editor rejects excess entries and never saves cancelled drafts', async () => {
      const { context, page } = await setup();
      try {
        await page.evaluate(() => display.openEditor(document.querySelector('nav a')));
        await page.getByLabel('Boards', { exact: true }).fill('demo test');
        await page.getByRole('button', { name: 'Cancel', exact: true }).click();
        assert.deepEqual(await page.evaluate(() => saved), []);
        await page.evaluate(() => display.openEditor(document.querySelector('nav a')));
        await page.getByLabel('Boards', { exact: true }).fill(Array(65).fill('x').join(' '));
        await page.getByRole('button', { name: 'Save board list' }).click();
        assert.match(await page.getByRole('status').textContent(), /at most 64/);
        assert.deepEqual(await page.evaluate(() => saved), []);
        await page.getByLabel('Boards', { exact: true }).fill('demo test');
        await page.getByRole('button', { name: 'Save board list' }).click();
        assert.deepEqual(await page.evaluate(() => saved), [{ customMenu: true, customMenuList: 'demo test' }]);
        assert.equal(await page.getByRole('dialog').count(), 0);
        assert.equal(await page.locator('.customBoardList').count(), 1);
      } finally { await context.close(); }
    });

    await t.test('page lifecycle and destroy restore dates and menus without further decoration', async () => {
      const { context, page } = await setup('UTC', { customMenu: true, customMenuList: 'demo test' });
      try {
        await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
        assert.equal(await page.locator('.customBoardList').count(), 0);
        assert.equal(await page.locator('#pi100 time').textContent(), '03/08/26(Sun)05:59:59');
        await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
        assert.equal(await page.locator('.customBoardList').count(), 1);
        assert.equal(await page.locator('#pi100 time').textContent(), '03/08/26(Sun)09:59:59');
        await page.evaluate(() => { display.destroy(); document.dispatchEvent(new Event('4chanSettingsSaved')); });
        assert.equal(await page.locator('.customBoardList').count(), 0);
        assert.equal(await page.locator('#pi100 time').textContent(), '03/08/26(Sun)05:59:59');
        assert.equal(await page.locator('time[title]').count(), 0);
      } finally { await context.close(); }
    });
  } finally { await browser.close(); }
});
