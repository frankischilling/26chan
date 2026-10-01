import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-catalog-filters-reference.json', import.meta.url)));
test.use({ javaScriptEnabled: true });
const catalog = '/filterui/catalog';
function localRules(filters) {
  return Object.fromEntries(Object.entries(filters).map(([index, rule]) => [index, {
    ...rule, boards: rule.boards.split(' ').map(board => board === 'demo' ? 'filterui' : board).join(' '),
  }]));
}
async function prepare(page, context, width = 1280, filters = null, stored = {}) {
  await page.setViewportSize({ width, height: 900 });
  await context.addInitScript(({ filters, stored }) => {
    localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
    if (filters) localStorage.setItem('catalog-filters', JSON.stringify(filters));
    for (const [key, value] of Object.entries(stored)) {
      if (key.startsWith('4chan-catalog-search')) sessionStorage.setItem(key, value);
      else localStorage.setItem(key, typeof value === 'string' ? value : JSON.stringify(value));
    }
  }, { filters, stored });
  await page.goto(catalog);
  await expect(page.locator('#filters-ctrl')).toBeVisible();
}
async function state(page) {
  return page.evaluate(() => ({
    cards: [...document.querySelectorAll('#threads > .thread')].map(node => ({ id: node.id,
      highlighted: node.querySelector('.thumb').classList.contains('hl'), border: node.querySelector('.thumb').style.borderColor,
      color: node.querySelector('.teaser')?.style.color ?? '',
    })),
    filtered: ['filtered-label', 'filtered-label-bottom'].map(id => getComputedStyle(document.getElementById(id)).display),
    counts: ['filtered-count', 'filtered-count-bottom'].map(id => document.getElementById(id).textContent),
  }));
}
for (const row of reference.matches) {
  test(`catalog filters match public client ${row.label}`, async ({ page, context }) => {
    const errors = [], failures = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('requestfailed', request => failures.push(request.url()));
    const stored = Object.fromEntries(Object.entries(row.stored).map(([key, value]) => [key.replace(/-demo$/, '-filterui'),
      key === '4chan-catalog-search-board' && value === 'demo' ? 'filterui' : value]));
    await prepare(page, context, 1280, localRules(row.filters), stored);
    await expect.poll(() => state(page)).toEqual(row.value);
    expect(errors).toEqual([]); expect(failures).toEqual([]);
  });
}

async function editorState(page, label, width) {
  const actual = await page.evaluate(() => ({
    open: document.getElementById('filters').open,
    backdrop: document.getElementById('filters').open,
    search: document.getElementById('filters-search').value,
    rows: [...document.getElementById('filter-list').children].map(row => ({
      pattern: row.querySelector('.filter-pattern').value, boards: row.querySelector('.filter-boards').value,
      active: row.querySelector('.filter-active').checked, hidden: row.querySelector('.filter-hide').checked,
      top: row.querySelector('.filter-top').checked, display: row.style.display,
      color: row.querySelector('.filter-color').style.backgroundColor,
      noColor: row.querySelector('.filter-color').hasAttribute('data-nocolor'), hits: row.querySelector('.filter-hits').textContent,
    })),
    palette: document.getElementById('filter-palette').open,
    help: document.getElementById('filters-protip').open,
    stored: localStorage.getItem('catalog-filters'),
  }));
  const expected = reference.states.find(row => row.label === label && row.width === width);
  const localized = { ...expected, rows: expected.rows.map(row => ({ ...row, boards: row.boards === 'demo' ? 'filterui' : row.boards })),
    stored: expected.stored === null ? null : JSON.stringify(localRules(JSON.parse(expected.stored))),
  };
  expect({ label, width, ...actual }).toEqual(localized);
}
for (const width of [1280, 390]) {
  test(`catalog filter editor follows all public states at ${width}`, async ({ page, context }) => {
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await prepare(page, context, width);
    await page.locator('#filters-ctrl').click(); await editorState(page, 'empty-open', width);
    await page.locator('#filters-add').click(); await editorState(page, 'added-empty', width);
    await page.locator('.filter-pattern').fill('sheet'); await page.locator('.filter-boards').fill('filterui');
    await page.locator('.filter-hide').check(); await page.locator('.filter-top').check(); await editorState(page, 'both-hide-top', width);
    await page.locator('.filter-color').click(); await editorState(page, 'palette-open', width);
    await page.locator('#filter-color-table tbody .clickbox').first().click(); await editorState(page, 'palette-selected', width);
    await page.locator('#filters-save').click(); await expect(page.locator('#filters')).not.toBeVisible(); await editorState(page, 'saved-closed', width);
    await page.locator('#filters-ctrl').click(); await editorState(page, 'reopened-saved', width);
    await page.locator('#filters-add').click(); await page.locator('.filter-pattern').last().fill('folds'); await editorState(page, 'second-added', width);
    await page.locator('[data-up]').last().click(); await editorState(page, 'moved-up', width);
    if (width === 1280) {
      await page.locator('#filters-search').fill('sheet'); await page.locator('#filters-search').press('ArrowRight'); await editorState(page, 'row-search', width);
      await page.locator('#filters-search').press('Escape'); await editorState(page, 'row-search-escape', width);
    }
    await page.locator('[data-target]').first().click(); await editorState(page, 'deleted-first', width);
    await page.locator('#filters-help-open').click(); await editorState(page, 'help-open', width);
    await page.locator('#filters-help-close').click(); await editorState(page, 'help-closed', width);
    await page.locator('#filters-close').click(); await editorState(page, 'dismissed', width);
    expect(errors).toEqual([]);
  });
}
for (const row of reference.cases) {
  test(`catalog filter editor styles in ${row.theme} at ${row.width} density ${row.density}`, async ({ browser }) => {
    const context = await browser.newContext({ deviceScaleFactor: row.density });
    const page = await context.newPage();
    try {
    await context.addCookies([{ name: 'board-theme-ws', value: row.theme, url: 'http://127.0.0.1:3000' }]);
      await prepare(page, context, row.width, { 0: { active: 1, pattern: 'sheet', boards: '', hidden: 0, top: 0, color: '#E0B0FF' } });
      await page.locator('#filters-ctrl').click(); await page.mouse.move(0, 0);
      const actual = await page.evaluate(({ values: expected, icons }) => Object.fromEntries(Object.entries(expected).map(([name, values]) => {
        const selector = { panel: '#filters', header: '#filters > .panelHeader', search: '#filters-search', table: '#filter-table',
          heading: '#filter-table th', cell: '#filter-list td', pattern: '.filter-pattern', boards: '.filter-boards', hits: '.filter-hits', help: '#filters-help-open' }[name];
        const style = getComputedStyle(document.querySelector(selector));
        const observed = Object.fromEntries(Object.keys(values).map(key => [key, style[key]]));
        if (name === 'help') {
          const path = new URL(style.backgroundImage.slice(5, -2)).pathname;
          const asset = icons.assets.find(asset => path === icons.local_base + asset.name);
          if (asset) observed.backgroundImage = 'url("' + asset.url + '")';
        }
        return [name, observed];
      })), { values: row.values, icons: reference.icons });
      expect(actual).toEqual(row.values);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    } finally { await context.close(); }
  });
}
