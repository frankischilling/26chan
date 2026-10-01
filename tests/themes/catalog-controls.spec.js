import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-catalog-ui-reference.json', import.meta.url)));
test.use({ javaScriptEnabled: true });
const catalog = '/controlui/catalog';
async function state(page, label) {
  const actual = await page.evaluate(() => ({
    display: getComputedStyle(document.getElementById('qf-cnt')).display,
    active: document.getElementById('qf-ctrl').classList.contains('active'),
    value: document.getElementById('qf-box').value,
    focus: getComputedStyle(document.getElementById('qf-cnt')).display === 'none' ? null : document.activeElement.id,
    labels: ['search-label', 'search-label-bottom'].map(id => getComputedStyle(document.getElementById(id)).display),
    terms: ['search-term', 'search-term-bottom'].map(id => document.getElementById(id).textContent),
    cards: [...document.querySelectorAll('#threads > .thread')].map(node => node.id),
    stored: { query: sessionStorage.getItem('4chan-catalog-search'), board: sessionStorage.getItem('4chan-catalog-search-board') },
  }));
  if (actual.stored.board === 'controlui') actual.stored.board = 'demo';
  const expected = reference.behavior.find(row => row.label === label && row.width === page.viewportSize().width);
  expect({ label, width: page.viewportSize().width, ...actual }).toEqual(expected);
}

for (const width of [1280, 390]) {
  test(`catalog Search matches all pinned client transitions at ${width}`, async ({ page, context }) => {
    const errors = [], requests = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('request', request => requests.push(request.url()));
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
    await context.addCookies([{ name: 'board-theme-ws', value: 'yotsuba-b', url: 'http://127.0.0.1:3000' }]);
    await page.setViewportSize({ width, height: 900 });
    await page.clock.install({ time: new Date(reference.environment.clock) });
    await page.goto(catalog);
    await page.clock.pauseAt(new Date(reference.environment.paused_at));
    await state(page, 'initial');
    await page.locator('#qf-ctrl').click(); await state(page, 'opened');
    await page.locator('#qf-box').fill('crane'); await page.locator('#qf-box').press('ArrowRight');
    await page.clock.runFor(249); await state(page, 'debounce-249');
    await page.clock.runFor(1); await state(page, 'debounce-250');
    await page.locator('#qf-box').press('Escape'); await state(page, 'escape');
    await page.locator('#qf-ctrl').click(); await state(page, 'reopened');
    await page.locator('#qf-box').fill('boat'); await page.locator('#qf-box').press('ArrowRight');
    await page.clock.runFor(250); await page.locator('#qf-clear').click(); await state(page, 'close-button');
    await page.keyboard.press('s'); await state(page, 'shortcut-open');
    await page.locator('#qf-box').fill('crane'); await page.locator('#qf-box').press('ArrowRight');
    await page.clock.runFor(250); await page.locator('#qf-box').evaluate(node => node.blur());
    await page.keyboard.press('s'); await state(page, 'shortcut-clears-active-input');
    await page.locator('#qf-box').press('ArrowRight'); await page.clock.runFor(250); await state(page, 'cleared-input-applied');
    await page.locator('#qf-box').fill('boat'); await page.locator('#qf-box').press('ArrowRight');
    await page.clock.runFor(100); await page.locator('#qf-ctrl').click(); await state(page, 'pending-close');
    await page.clock.runFor(150); await state(page, 'pending-close-250');
    expect(errors).toEqual([]);
    expect(requests.every(url => new URL(url).origin === 'http://127.0.0.1:3000')).toBe(true);
  });

  for (const [label, hash, saved] of [['same-board-session', '', { query: 'crane', board: 'controlui' }],
    ['different-board-session', '', { query: 'crane', board: 'other' }], ['fragment-search', '#s=Owned+boat', null]]) {
    test(`catalog Search restores ${label} at ${width}`, async ({ page, context }) => {
      await page.setViewportSize({ width, height: 900 });
      await context.addInitScript(saved => {
        localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
        if (saved) { sessionStorage.setItem('4chan-catalog-search', saved.query); sessionStorage.setItem('4chan-catalog-search-board', saved.board); }
      }, saved);
      await page.goto(catalog + hash);
      await state(page, label);
    });
  }
  test(`catalog Search matches pinned input, composition and Enter at ${width}`, async ({ page, context }) => {
    await page.setViewportSize({ width, height: 900 });
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
    await page.clock.install({ time: new Date(reference.environment.clock) });
    await page.goto(catalog); await page.clock.pauseAt(new Date(reference.environment.paused_at));
    await page.locator('#qf-ctrl').click(); await page.locator('#qf-box').fill('crane');
    await page.clock.runFor(250); await state(page, 'input-without-keyup');
    await page.locator('#qf-box').dispatchEvent('compositionstart');
    await page.locator('#qf-box').fill('boat'); await page.locator('#qf-box').press('ArrowRight');
    await page.clock.runFor(250); await state(page, 'composition-keyup');
    await page.locator('#qf-box').dispatchEvent('compositionend');
    await page.clock.runFor(250); await state(page, 'composition-end-without-keyup');
    await page.locator('#qf-box').fill('crane'); await page.locator('#qf-box').press('Enter');
    await state(page, 'enter-immediate'); await page.clock.runFor(250); await state(page, 'enter-250');
  });
  for (const [label, modifiers] of [['control-shortcut', { ctrlKey: true }], ['alt-shortcut', { altKey: true }], ['shift-shortcut', { shiftKey: true }]]) {
    test(`catalog Search ignores ${label} keydown at ${width}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 }); await page.goto(catalog);
      await page.evaluate(modifiers => document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 's', keyCode: 83, bubbles: true, cancelable: true, ...modifiers })), modifiers);
      await state(page, label);
    });
  }
}

for (const row of reference.cases) {
  test(`pinned catalog control styles in ${row.theme} at ${row.width} ${row.state}`, async ({ page, context }) => {
    await context.addCookies([{ name: 'board-theme-ws', value: row.theme, url: 'http://127.0.0.1:3000' }]);
    await page.setViewportSize({ width: row.width, height: 900 });
    await page.goto(catalog);
    if (row.state === 'open') await page.locator('#qf-ctrl').click();
    await page.mouse.move(0, 0);
    const actual = await page.evaluate(values => Object.fromEntries(Object.entries(values).map(([name, expected]) => {
      const selector = ({ settings: '#settings', wrapper: '#qf-ctrl', button: '#qf-ctrl', brackets: '#qf-ctrl', container: '#qf-cnt', input: '#qf-box', clear: '#qf-clear' })[name];
      const element = document.querySelector(selector);
      const node = name === 'wrapper' || name === 'brackets' ? element.parentElement : element;
      if (name === 'brackets') return [name, { before: getComputedStyle(node, '::before').content, after: getComputedStyle(node, '::after').content }];
      const style = getComputedStyle(node);
      const observed = Object.fromEntries(Object.keys(expected).map(key => [key, style[key]]));
      if (observed.backgroundImage?.startsWith('url(')) {
        const file = /\/([^/]+)"\)$/.exec(observed.backgroundImage)?.[1];
        if (['buttonfade.png', 'buttonfade-blue.png', 'buttonfade-dark.png'].includes(file)) observed.backgroundImage = `url("https://reference.invalid/image/${file}")`;
      }
      return [name, observed];
    })), row.values);
    expect(actual).toEqual(row.values);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  });
}
