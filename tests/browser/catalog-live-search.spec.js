import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const contract = JSON.parse(await readFile(new URL('../fixtures/catalog-search-cases.json', import.meta.url), 'utf8'));
const catalog = '/test/catalog';
const termKey = '4chan-catalog-search';
const boardKey = '4chan-catalog-search-board';
const query = page => new URL(page.url()).searchParams.get('q') || '';

test('live search debounces the latest input, waits for composition and clears on Escape', async ({ page }) => {
  await page.clock.install({ time: new Date('2026-09-13T12:00:00Z') });
  await page.goto(`${catalog}?q=`);
  await page.clock.pauseAt(new Date('2026-09-13T13:00:00Z'));
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations += 1; });
  const input = page.locator('#qf-box');
  await input.fill('Alpha');
  await page.clock.runFor(249);
  expect(query(page)).toBe('');
  await input.fill('Beta');
  await page.clock.runFor(249);
  expect(query(page)).toBe('');
  await page.clock.runFor(1);
  expect(query(page)).toBe('Beta');
  await input.dispatchEvent('compositionstart');
  await input.fill('Gamma');
  await page.clock.runFor(500);
  expect(query(page)).toBe('Beta');
  await input.dispatchEvent('compositionend');
  await page.clock.runFor(250);
  expect(query(page)).toBe('Gamma');
  await input.press('Escape');
  expect(query(page)).toBe('');
  await expect(input).toHaveValue('');
  expect(navigations).toBe(0);
});

test('search sessions are tab-local, clear on a board change and support bounded fragment links', async ({ page, context }) => {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(catalog);
  await page.locator('#qf-box').fill('sessionneedle');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  expect(await page.evaluate(key => sessionStorage.getItem(key), termKey)).toBe('sessionneedle');
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations += 1; });
  await page.goto(catalog);
  await expect(page.locator('#qf-box')).toHaveValue('sessionneedle');
  expect(navigations).toBe(1);
  const fresh = await context.newPage();
  await fresh.goto(catalog);
  await expect(fresh.locator('#qf-box')).toHaveValue('');
  await page.goto(`${catalog}#threads`);
  await expect(page.locator('#qf-box')).toHaveValue('');
  await page.goto('/demo/catalog');
  await expect(page.locator('#qf-box')).toHaveValue('');
  expect(await page.evaluate(keys => keys.map(key => sessionStorage.getItem(key)), [termKey, boardKey])).toEqual([null, null]);
  await page.goto(`${catalog}#s=Alpha+%5B.*%5D`);
  await expect(page.locator('#qf-box')).toHaveValue('Alpha [.*]');
  await page.goto(`${catalog}?q=explicit#s=other`);
  await expect(page.locator('#qf-box')).toHaveValue('explicit');
  await page.goto(`${catalog}#s=%E0%A4%A`);
  await expect(page.locator('#qf-box')).toHaveValue('');
  await page.evaluate(({ termKey, boardKey }) => {
    sessionStorage.setItem(termKey, 'x'.repeat(129));
    sessionStorage.setItem(boardKey, 'test');
  }, { termKey, boardKey });
  await page.goto(catalog);
  await expect(page.locator('#qf-box')).toHaveValue('');
  expect(await page.evaluate(key => sessionStorage.getItem(key), termKey)).toBeNull();
  expect(errors).toEqual([]);
});

test('the actual live matcher passes every shared operator and case example without navigating', async ({ page }) => {
  await page.clock.install({ time: new Date('2026-09-13T12:00:00Z') });
  await page.goto(`${catalog}?q=`);
  await page.clock.pauseAt(new Date('2026-09-13T13:00:00Z'));
  await page.evaluate(async cases => {
    const form = document.getElementById('ctrl').cloneNode(true);
    const threads = document.createElement('div');
    threads.id = 'threads';
    threads.className = 'catalog extended-small';
    const hidden = document.createElement('template');
    hidden.id = 'catalogFiltered';
    for (const [index, entry] of cases.entries()) {
      const card = document.createElement('section');
      card.className = 'thread';
      card.id = `thread-${index + 1}`;
      Object.assign(card.dataset, { threadId: String(index + 1), bumped: '100', latestReply: '', replies: '0', sticky: 'false' });
      const link = document.createElement('a');
      link.className = 'catalogThumb';
      Object.assign(link.dataset, { searchSubject: entry.text, searchComment: entry.text, searchFile: '', hasFile: 'false' });
      const teaser = document.createElement('div');
      teaser.className = 'teaser';
      teaser.textContent = entry.text;
      card.append(link, teaser);
      threads.append(card);
    }
    document.body.replaceChildren(form, threads, hidden);
    await new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = '/static/catalog-preferences.v1.js';
      script.onload = resolve;
      script.onerror = reject;
      document.body.append(script);
    });
  }, contract.cases);
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations += 1; });
  for (const [index, entry] of contract.cases.entries()) {
    await page.locator('#qf-box').fill(entry.query);
    await page.clock.runFor(250);
    expect(query(page)).toBe(entry.query);
    await expect(page.locator(`#thread-${index + 1}`)).toHaveCount(entry.matches ? 1 : 0);
  }
  expect(navigations).toBe(0);
});

test('inert filtered cards do not request their images until shown', async ({ page }) => {
  const response = await page.goto(`${catalog}?q=`);
  const csp = response.headers()['content-security-policy'];
  const healthy = await page.evaluate(() => new Promise((resolve, reject) => {
    const image = document.createElement('img');
    image.onload = () => resolve(image.naturalWidth);
    image.onerror = reject;
    image.src = '/static/catalog/nofile.png?allowed-probe';
    document.body.append(image);
  }));
  expect(healthy).toBe(77);
  const card = (id, text, image = '') => `<section class="thread" id="thread-${id}" data-thread-id="${id}" data-bumped="100" data-latest-reply="" data-replies="0" data-sticky="false"><a class="catalogThumb" data-search-subject="${text}" data-search-comment="${text}" data-search-file="" data-has-file="false">${image}</a><div class="teaser">${text}</div></section>`;
  const image = '<img id="thumb-2" src="/static/catalog/nofile.png?hidden-probe" width="77" height="13" data-small-width="77" data-small-height="13" data-large-width="77" data-large-height="13">';
  const html = `<!doctype html><form id="ctrl" action="/test/catalog" method="get"><select id="order-ctrl" name="order"><option value="alt">Bump</option></select><select id="size-ctrl" name="size"><option value="small">Small</option></select><select id="teaser-ctrl" name="teaser"><option value="on">On</option></select><input id="qf-box" name="q" type="search" value="visible"><button>Apply</button><a id="catalog-reset" href="/test/catalog">Reset</a></form><div id="threads" class="catalog extended-small">${card(1, 'visible')}</div><template id="catalogFiltered">${card(2, 'hidden', image)}</template><script src="/static/catalog-preferences.v1.js" defer></script>`;
  await page.route('**/test/catalog?q=visible', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: html }));
  let requests = 0;
  page.on('request', request => { if (request.url().includes('hidden-probe')) requests += 1; });
  await page.goto(`${catalog}?q=visible`);
  expect(requests).toBe(0);
  await page.evaluate(() => { window.hiddenCard = document.getElementById('catalogFiltered').content.querySelector('.thread'); });
  const shown = page.waitForResponse(response => response.url().includes('hidden-probe'));
  await page.locator('#qf-box').fill('hidden');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  expect((await shown).status()).toBe(200);
  await expect(page.locator('#thread-2')).toBeVisible();
  expect(await page.evaluate(() => document.getElementById('thread-2') === window.hiddenCard)).toBe(true);
  expect(requests).toBe(1);
});

test('live search stays usable without storage and enforces scalar-value bounds', async ({ page, context }) => {
  await context.addInitScript(() => {
    for (const name of ['getItem', 'setItem', 'removeItem']) {
      Object.defineProperty(Storage.prototype, name, { value() { throw new DOMException('Storage unavailable', 'SecurityError'); } });
    }
  });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(catalog);
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations += 1; });
  const input = page.locator('#qf-box');
  const limit = String.fromCodePoint(0x1f600).repeat(128);
  await input.fill(limit);
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  expect(query(page)).toBe(limit);
  await input.fill(`${limit}a`);
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  expect(query(page)).toBe(limit);
  expect(await input.evaluate(node => node.validationMessage)).toContain('128 characters');
  await input.press('Escape');
  expect(query(page)).toBe('');
  expect(await input.evaluate(node => node.validationMessage)).toBe('');
  expect(navigations).toBe(0);
  expect(errors).toEqual([]);
});

test('empty catalogs distinguish no threads from no matches across every clear control', async ({ page }) => {
  const response = await page.goto(`${catalog}?q=`);
  const csp = response.headers()['content-security-policy'];
  const html = initial => `<!doctype html><form id="ctrl" action="/test/catalog" method="get"><select id="order-ctrl" name="order"><option value="alt">Bump</option></select><select id="size-ctrl" name="size"><option value="small">Small</option></select><select id="teaser-ctrl" name="teaser"><option value="on">On</option></select><input id="qf-box" name="q" type="search" value="${initial}"><button>Apply</button><a id="catalog-reset" href="/test/catalog">Reset</a></form><div id="threads" class="catalog extended-small"><p class="empty">${initial ? 'No matching threads. <a href="/test/catalog">Show all threads</a>.' : 'No threads yet. <a href="/test/#postForm">Start the first thread</a>.'}</p></div><template id="catalogFiltered"></template><script src="/static/catalog-preferences.v1.js" defer></script>`;
  await page.route('**/test/catalog?empty-state=*', route => {
    const initial = new URL(route.request().url()).searchParams.get('q');
    return route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: html(initial) });
  });
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations += 1; });
  for (const initial of ['', 'missing']) {
    for (const control of ['Apply', 'Escape', 'Reset', 'Show all threads']) {
      await page.goto(`${catalog}?empty-state=regression&q=${initial}`);
      navigations = 0;
      const input = page.locator('#qf-box');
      const empty = page.locator('#threads > .empty');
      await expect(empty).toHaveText(initial ? 'No matching threads. Show all threads.' : 'No threads yet. Start the first thread.');
      if (!initial) {
        await input.fill('missing');
        await page.getByRole('button', { name: 'Apply', exact: true }).click();
      }
      await expect(empty).toHaveText('No matching threads. Show all threads.');
      if (control === 'Apply') {
        await input.fill('');
        await page.getByRole('button', { name: 'Apply', exact: true }).click();
      } else if (control === 'Escape') {
        await input.press('Escape');
      } else {
        await page.getByRole('link', { name: control, exact: true }).click();
      }
      await expect(input).toHaveValue('');
      await expect(empty).toHaveText('No threads yet. Start the first thread.');
      await expect(empty.getByRole('link', { name: 'Start the first thread', exact: true })).toHaveAttribute('href', /\/test\/#postForm$/);
      await expect(empty.getByRole('link', { name: 'Show all threads', exact: true })).toHaveCount(0);
      expect(query(page)).toBe('');
      expect(navigations).toBe(0);
      await input.fill('missing again');
      await page.getByRole('button', { name: 'Apply', exact: true }).click();
      await expect(empty).toHaveText('No matching threads. Show all threads.');
      await empty.getByRole('link', { name: 'Show all threads', exact: true }).click();
      await expect(empty).toHaveText('No threads yet. Start the first thread.');
      expect(query(page)).toBe('');
      expect(navigations).toBe(0);
    }
  }
});
