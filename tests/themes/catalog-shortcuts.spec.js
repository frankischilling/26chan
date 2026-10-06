import { test, expect } from '../helpers/visual-diagnostics.js';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-catalog-settings-reference.json', import.meta.url), 'utf8'));
test.use({ javaScriptEnabled: true });
const catalog = '/settingsui/catalog';
async function prepare(page, context, width, disabled = false) {
  await page.setViewportSize({ width, height: 900 });
  await context.addInitScript(disabled => {
    localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
    if (disabled) localStorage.setItem('catalog-theme', JSON.stringify({ nobinds: true }));
  }, disabled);
  await page.goto(catalog);
  await expect(page.locator('#qf-ctrl')).toBeVisible();
  await page.evaluate(() => {
    for (const [tag, id] of [['input', 'owned-input'], ['textarea', 'owned-textarea'], ['button', 'owned-button'], ['a', 'owned-link']]) {
      const node = document.createElement(tag); node.id = id; node.textContent = 'Owned keyboard target';
      if (tag === 'button') node.type = 'button';
      if (tag === 'a') node.href = '#bottom';
      document.body.append(node);
    }
  });
}
async function dispatch(page, row) {
  return page.evaluate(({ type, target, code, modifiers }) => {
    const node = target === 'body' ? document.body : document.getElementById(target);
    const event = new KeyboardEvent(type, { key: String.fromCharCode(code).toLowerCase(), keyCode: code, bubbles: true, cancelable: true, ...modifiers });
    node.dispatchEvent(event);
    return { type, target, code, modifiers, prevented: event.defaultPrevented };
  }, row.event);
}
async function state(page, row, event) {
  const actual = await page.evaluate(() => ({
    shown: getComputedStyle(document.getElementById('qf-cnt')).display,
    active: document.getElementById('qf-ctrl').classList.contains('active'),
    input: document.getElementById('qf-box').value,
    order: document.getElementById('order-ctrl').value,
    cards: [...document.querySelectorAll('#threads > .thread')].map(node => node.id),
  }));
  expect({ label: row.label, width: page.viewportSize().width, event, ...actual }).toEqual(row);
}
for (const width of [1280, 390]) {
  for (const row of reference.shortcuts.filter(row => row.width === width && row.event.type === 'keydown')) {
    test(`catalog S matches public ${row.event.target} and modifiers ${row.label} at ${width}`, async ({ page, context }) => {
      const errors = []; page.on('pageerror', error => errors.push(error.message));
      await prepare(page, context, width);
      await state(page, row, await dispatch(page, row));
      const released = reference.shortcuts.find(value => value.width === width && value.label === row.label.replace('keydown', 'keyup'));
      await state(page, released, await dispatch(page, released));
      expect(errors).toEqual([]);
    });
  }
  test(`catalog nobinds and X cycle match the public client at ${width}`, async ({ page, context, browser }) => {
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await prepare(page, context, width);
    for (const row of reference.shortcuts.filter(row => row.width === width && row.label.startsWith('cycle-'))) {
      await state(page, row, await dispatch(page, row));
    }
    const disabledContext = await browser.newContext();
    try {
      const disabledPage = await disabledContext.newPage(); disabledPage.on('pageerror', error => errors.push(error.message));
      await prepare(disabledPage, disabledContext, width, true);
      const row = reference.shortcuts.find(row => row.width === width && row.label === 'disabled-keyup');
      await state(disabledPage, row, await dispatch(disabledPage, row));
    } finally { await disabledContext.close(); }
    expect(errors).toEqual([]);
  });
}
for (const row of reference.reloads) {
  test(`catalog R matches public ${row.label} at ${row.width}`, async ({ page, context }) => {
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await prepare(page, context, row.width, row.label === 'disabled');
    const original = page.url(); let navigations = 0;
    page.on('framenavigated', frame => { if (frame === page.mainFrame()) navigations++; });
    const event = await dispatch(page, row);
    if (row.navigations) await expect.poll(() => navigations).toBe(row.navigations);
    else await page.waitForTimeout(150);
    await page.waitForLoadState('load');
    expect({ label: row.label, width: row.width, event, navigations, samePage: page.url() === original }).toEqual(row);
    expect(errors).toEqual([]);
  });
}
