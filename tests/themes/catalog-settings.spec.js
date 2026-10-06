import { watcherSettingsOpener } from '../browser/helpers/watcher-settings.js';
import { test, expect } from '../helpers/visual-diagnostics.js';
import { readFile } from 'node:fs/promises';
const reference = JSON.parse(await readFile(new URL('../../docs/public-catalog-settings-reference.json', import.meta.url), 'utf8'));
test.use({ javaScriptEnabled: true });
const catalog = '/settingsui/catalog';
async function open(page, width) {
  const navigation = page.getByRole('navigation', { name: 'Persistent board navigation', exact: true });
  if (await navigation.isVisible()) await navigation.getByRole('button', { name: 'Settings', exact: true }).click();
  else await watcherSettingsOpener(page).click();
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toBeVisible();
}
async function state(page, label) {
  const actual = await page.evaluate(() => {
    const panel = document.getElementById('theme');
    return {
      shown: !!panel?.open,
      fields: panel ? ['nobinds', 'nospoiler', 'newtab', 'tw', 'ddn'].map(key => {
        const input = document.getElementById('theme-' + key);
        return { key, checked: input.checked, rowDisplay: getComputedStyle(input.closest('li')).display };
      }) : [],
      css: panel ? document.getElementById('theme-css').value : null,
      spoilerClass: document.body.classList.contains('reveal-img-spoilers'),
      links: [...document.querySelectorAll('#threads > .thread > .catalogThumb')].map(link => ({ id: link.parentElement.id, target: link.getAttribute('target') })),
      stored: { theme: localStorage.getItem('catalog-theme'), settings: localStorage.getItem('4chan-settings') },
    };
  });
  const expected = [...reference.states, ...reference.nativeDefaults].find(row => row.label === label && row.width === page.viewportSize().width);
  // The native modal focuses the first visible checkbox. The reference's
  // auxiliary anchor position does not establish original-page focus timing.
  const { focus, ...observed } = expected;
  expect({ label, width: page.viewportSize().width, ...actual }).toEqual(observed);
}
for (const width of [1280, 390]) for (const [label, settings] of [
  ['absent', null], ['empty', {}], ['explicit-off', { threadWatcher: false, dropDownNav: false }],
  ['explicit-on', { threadWatcher: true, dropDownNav: true }],
]) {
  test(`catalog Settings matches recorded ${label} native defaults at ${width}`, async ({ page }) => {
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(settings => {
      if (settings === null) localStorage.removeItem('4chan-settings'); else localStorage.setItem('4chan-settings', JSON.stringify(settings));
    }, settings);
    await page.setViewportSize({ width, height: 900 }); await page.goto(catalog); await open(page, width);
    await state(page, label + '-opened');
    await page.locator('#theme-save').click(); await expect(page.locator('#theme')).toBeHidden(); await state(page, label + '-saved');
    expect(errors).toEqual([]);
  });
}
for (const width of [1280, 390]) {
  test(`catalog Settings matches recorded fields and persistence at ${width}`, async ({ page, context }) => {
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
    await page.setViewportSize({ width, height: 900 }); await page.goto(catalog);
    await expect(page.locator('#qf-ctrl')).toBeVisible();
    await state(page, 'initial'); await open(page, width); await state(page, 'opened-defaults');
    await expect(page.locator(width <= 480 ? '#theme-nospoiler' : '#theme-nobinds')).toBeFocused();
    await page.locator('#theme-nospoiler').check(); await page.locator('#theme-newtab').check();
    await page.locator('#theme-nobinds').evaluate(node => node.checked = true); await state(page, 'checked-unsaved');
    await page.locator('#theme-save').click(); await expect(page.locator('#theme')).toBeHidden(); await state(page, 'saved-three-options');
    await expect(page.locator('#threads .catalogThumb').first()).toHaveAttribute('rel', 'noopener noreferrer');
    await open(page, width); await state(page, 'reopened-three-options');
    for (const key of ['nobinds', 'nospoiler', 'newtab']) await page.locator('#theme-' + key).evaluate(node => node.checked = false);
    await page.locator('#theme-save').click(); await expect(page.locator('#theme')).toBeHidden(); await state(page, 'saved-empty-theme');
    await open(page, width); await page.locator('#theme-css').fill('.teaser { color: #008000; }'); await state(page, 'css-unsaved');
    await page.locator('#theme-close').click(); await state(page, 'closed-unsaved');
    await open(page, width); await state(page, 'reopened-unsaved-css');
    await page.locator('#theme-save').click(); await expect(page.locator('#theme')).toBeHidden(); await state(page, 'saved-css');
    await expect(page.locator('#threads .teaser').first()).toHaveCSS('color', 'rgb(0, 128, 0)');
    await open(page, width); await state(page, 'reopened-css');
    await page.locator('#theme-css').fill(''); await page.locator('#theme-save').click(); await expect(page.locator('#theme')).toBeHidden(); await state(page, 'cleared-css');
    expect(errors).toEqual([]);
  });
}
async function panelStyle(page) {
  return page.evaluate(() => Object.fromEntries([
    ['panel', '#theme', ['width', 'fontFamily', 'fontSize', 'color', 'backgroundColor', 'padding', 'borderWidth', 'borderColor', 'borderRadius', 'boxShadow']],
    ['header', '#theme .panelHeader', ['fontFamily', 'fontSize', 'fontWeight', 'lineHeight', 'margin', 'padding', 'borderBottomWidth', 'borderBottomColor', 'textAlign']],
    ['heading', '#theme h4', ['fontSize', 'fontWeight', 'margin', 'padding']],
    ['list', '#theme ul.clickset', ['margin', 'padding', 'listStyleType']],
    ['row', '#theme ul.clickset li', ['margin', 'padding', 'lineHeight']],
    ['checkbox', '#theme-nospoiler', ['margin', 'padding']],
    ['css', '#theme-css', ['width', 'height', 'fontFamily', 'fontSize', 'margin', 'padding', 'borderWidth', 'borderColor', 'color', 'backgroundColor', 'boxSizing']],
    ['actions', '#theme-btns', ['margin', 'padding', 'textAlign']],
    ['submit', '#theme-save', ['fontFamily', 'fontSize', 'padding', 'margin']],
  ].map(([name, selector, properties]) => {
    const style = getComputedStyle(document.querySelector(selector));
    return [name, Object.fromEntries(properties.map(property => [property, style[property]]))];
  })));
}
for (const row of reference.cases) {
  test(`catalog Settings matches public ${row.theme} styles at ${row.width}`, async ({ page, context }) => {
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await context.addCookies([{ name: 'board-theme-ws', value: row.theme, url: 'http://127.0.0.1:3000' }]);
    await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
    await page.setViewportSize({ width: row.width, height: 900 }); await page.goto(catalog); await open(page, row.width);
    await page.mouse.move(0, 0);
    expect(await panelStyle(page)).toEqual(row.values); expect(errors).toEqual([]);
  });
}
