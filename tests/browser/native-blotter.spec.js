import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { asciiFontFamily } from '../helpers/ascii-font-family.mjs';
const assets = new URL('../../apps/public/static/', import.meta.url);
const script = await readFile(new URL('native-blotter.v1.js', assets), 'utf8');
const css = await readFile(new URL('board.css', assets), 'utf8');
const geometry = JSON.parse(await readFile(new URL('../fixtures/native-blotter-geometry.json', import.meta.url), 'utf8'));
const commonTheme = await readFile(new URL('themes/common.css', assets), 'utf8');
const themeStyles = Object.fromEntries(await Promise.all(geometry.themes.map(async ({ theme }) => [theme, commonTheme + (theme === 'yotsuba' ? '' : await readFile(new URL(`themes/${theme}.css`, assets), 'utf8'))])));
const hostile = '<img src=x onerror="window.blotterInjected=1"> & <script>window.blotterInjected=1</script>';
const escape = value => value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
function preview(timestamp, content = hostile) {
  return `<table id="blotter" class="desktop" aria-label="Blotter"><thead><tr><td colspan="2"><hr class="aboveMidAd"></td></tr></thead><tbody id="blotter-msgs"><tr><td class="blotter-date">10/09/26</td><td class="blotterMessage">${escape(content)}</td></tr></tbody><tfoot><tr><td colspan="2">[<a href="#" id="toggleBlotter" data-utc="${timestamp}" aria-controls="blotter-msgs">Hide</a>]<span id="blotter-all"> [<a href="/blotter" target="_blank" rel="noopener">Show All</a>]</span></td></tr></tfoot></table>`;
}
async function fixture(page, state = {}) {
  await page.context().route('http://blotter.test/**', route => {
    const url = new URL(route.request().url());
    if (url.pathname === '/native-blotter.js') return route.fulfill({ contentType: 'text/javascript', body: script });
    if (url.pathname === '/theme.css') return route.fulfill({ contentType: 'text/css', body: themeStyles[state.theme ?? 'yotsuba'] });
    if (url.pathname === '/reference.css') return route.fulfill({ contentType: 'text/css', body: geometry.themes.find(row => row.theme === state.theme).rules });
    if (url.pathname === '/board.css') return route.fulfill({ contentType: 'text/css', body: css });
    if (url.pathname === '/start.js') return route.fulfill({ contentType: 'text/javascript', body: "import {mountNativeBlotter} from '/native-blotter.js'; mountNativeBlotter(document);" });
    const standalone = url.pathname === '/blotter';
    const content = standalone ? `<section class="blotterPage"><h1>Blotter</h1><table class="blotterEntries"><thead><tr><th class="col-date">Date</th><th>Message</th></tr></thead><tbody><tr id="msg-3"><td class="col-date">10/09/26</td><td class="blotterMessage">${escape(hostile)}</td></tr></tbody></table></section>`
      : state.disabled || state.empty ? '' : preview(state.timestamp ?? '200', state.content);
    return route.fulfill({ contentType: 'text/html', headers: { 'Content-Security-Policy': "default-src 'none'; script-src 'self'; style-src 'self'" }, body: `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="${url.pathname === '/reference' ? '/reference.css' : '/board.css'}">${url.pathname === '/reference' ? '' : '<link rel="stylesheet" href="/theme.css">'}</head><body><main>${content}</main><script type="module" src="/start.js"></script></body></html>` });
  });
}
test('hide survives reload, show clears it, and newer publication reappears', async ({ page }) => {
  const state = { timestamp: '200' }; await fixture(page, state); await page.goto('/demo/');
  await page.getByRole('link', { name: 'Hide', exact: true }).focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#blotter-msgs')).toBeHidden(); await expect(page.locator('#blotter-all')).toBeHidden();
  await page.reload(); await expect(page.getByRole('link', { name: 'Show Blotter' })).toBeVisible();
  await page.getByRole('link', { name: 'Show Blotter' }).click();
  expect(await page.evaluate(() => localStorage.getItem('4chan-blotter'))).toBeNull();
  await page.getByRole('link', { name: 'Hide', exact: true }).click();
  state.timestamp = '201'; await page.reload(); await expect(page.locator('#blotter-msgs')).toBeVisible();
});
for (const flag of ['disabled', 'empty']) test(`${flag} preview omits controls`, async ({ page }) => {
  await fixture(page, { [flag]: true }); await page.goto('/demo/'); await expect(page.locator('#blotter')).toHaveCount(0);
});
test('hostile content remains literal and Show All navigates locally', async ({ page }) => {
  await fixture(page); await page.goto('/demo/');
  await expect(page.locator('.blotterMessage')).toHaveText(hostile);
  await expect(page.locator('#blotter img, #blotter script')).toHaveCount(0);
  const [popup] = await Promise.all([page.waitForEvent('popup'),
    Promise.resolve().then(() => page.getByRole('link', { name: 'Show All' }).click())]);
  await expect(popup).toHaveURL('http://blotter.test/blotter');
  expect(await popup.evaluate(() => window.opener)).toBeNull(); await popup.close();
  expect(await page.evaluate(() => window.blotterInjected)).toBeUndefined();
});
test('mobile follows desktop-only preview and wraps standalone messages', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 }); await fixture(page); await page.goto('/demo/');
  await expect(page.locator('#blotter')).toBeHidden(); await page.goto('/blotter');
  await expect(page.locator('.blotterMessage')).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});
test('storage denied still allows hide and show', async ({ page }) => {
  await page.addInitScript(() => Object.defineProperty(window, 'localStorage', { get() { throw new DOMException('Denied', 'SecurityError'); } }));
  await fixture(page); await page.goto('/demo/');
  await page.getByRole('link', { name: 'Hide', exact: true }).click();
  await page.getByRole('link', { name: 'Show Blotter' }).click(); await expect(page.locator('#blotter-msgs')).toBeVisible();
});
test('no JavaScript keeps messages and local full-page link usable', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false }); const page = await context.newPage();
  try { await fixture(page); await page.goto('http://blotter.test/demo/');
    await expect(page.locator('#blotter-msgs')).toBeVisible(); await expect(page.locator('#toggleBlotter')).toBeVisible();
    await expect(page.locator('#toggleBlotter')).toHaveAttribute('href', '#');
    const [popup] = await Promise.all([page.waitForEvent('popup'),
      Promise.resolve().then(() => page.getByRole('link', { name: 'Show All' }).click())]); await expect(popup.locator('.blotterEntries')).toBeVisible(); await popup.close();
  } finally { await context.close(); }
});

for (const { theme } of geometry.themes) for (const deviceScaleFactor of [1, 2]) {
  test(`pinned ${theme} preview geometry at DPR ${deviceScaleFactor}`, async ({ browser }) => {
    const context = await browser.newContext({ deviceScaleFactor, viewport: { width: 1280, height: 900 } });
    try {
      const page = await context.newPage(); await fixture(page, { theme, content: 'A short announcement.' });
      const measure = async () => {
        const measured = await page.locator('#blotter').evaluate(table => {
          const date = table.querySelector('.blotter-date'), cell = table.querySelector('.blotterMessage');
          const divider = table.querySelector('hr'), footer = table.querySelector('tfoot');
          const rect = table.getBoundingClientRect(), style = getComputedStyle(table);
          return { width: rect.width, height: rect.height, spacing: style.borderSpacing,
            dateWidth: date.getBoundingClientRect().width, dateAlign: getComputedStyle(date).textAlign,
            font: getComputedStyle(cell).fontSize, family: getComputedStyle(cell).fontFamily,
            padding: getComputedStyle(cell).padding, footerAlign: getComputedStyle(footer).textAlign,
            dividerWidth: divider.getBoundingClientRect().width, dividerHeight: divider.getBoundingClientRect().height,
            dividerMargin: getComputedStyle(divider).margin, dividerBorder: getComputedStyle(divider).borderTopStyle };
        });
        return { ...measured, family: asciiFontFamily(measured.family) };
      };
      for (const width of [1280, 481]) {
        await page.setViewportSize({ width, height: 900 });
        await page.goto('http://blotter.test/reference'); const reference = await measure();
        await page.goto('http://blotter.test/demo/'); expect(await measure()).toEqual(reference);
      }
      for (const width of [480, 375]) {
        await page.setViewportSize({ width, height: 812 }); await expect(page.locator('#blotter')).toBeHidden();
      }
    } finally { await context.close(); }
  });
}
