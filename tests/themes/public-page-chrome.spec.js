import { test, expect } from '../helpers/visual-diagnostics.js';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';

const reference = JSON.parse(await readFile(new URL('../../docs/public-page-chrome-reference.json', import.meta.url)));
for (const [id, properties] of Object.entries(reference.component_styles)) {
  assert.equal(id, createHash('sha256').update(JSON.stringify(properties)).digest('hex').slice(0, 16));
}
const cases = reference.cases.map(row => ({ ...row, styles: Object.fromEntries(Object.entries(row.styles).map(([name, id]) => {
  assert.ok(Object.hasOwn(reference.component_styles, id), `Missing recorded style ${id}`);
  return [name, reference.component_styles[id]];
})) }));
const origin = 'http://127.0.0.1:3000';
const selectors = {
  page: 'body', banner: '.boardBanner',
  desktop: '#boardNavDesktop', desktopLink: '#boardNavDesktop .boardList a',
  mobile: '#boardNavMobile', mobileSelect: '#boardSelectMobile', mobileLink: '#settingsWindowLinkMobile',
  title: '.boardTitle', subtitle: '.boardSubtitle', footer: '#boardNavDesktopFoot',
  footerLink: '#navbotright a', footerLinks: '#footer-links', footerText: '#absbot',
};

for (const row of cases) {
  test.describe(`${row.theme} ${row.mode} worksafe=${row.worksafe} ${row.width} DPR ${row.scale}`, () => {
    test.use({ javaScriptEnabled: true, viewport: { width: row.width, height: 900 }, deviceScaleFactor: row.scale });
    test('navigation, title and footer retain the recorded public component styles', async ({ page, context }) => {
      const errors = [], external = [];
      page.on('pageerror', error => errors.push(error.message));
      page.on('request', request => { if (new URL(request.url()).origin !== origin) external.push(request.url()); });
      await context.addCookies([{ name: row.worksafe ? 'board-theme-ws' : 'board-theme', value: row.theme, url: origin }]);
      await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
      await page.goto(`/chrome/${row.worksafe ? 'demo' : 'zed'}/${row.mode === 'catalog' ? 'catalog' : ''}`);
      await expect(page.locator('body')).toHaveAttribute('data-native-never-mobile', 'false');
      await expect(page.locator('#settingsWindowLink')).toHaveAttribute('data-native-settings-ready', '');
      await page.mouse.move(0, 850);
      const actual = await page.evaluate(({ selectors, expected }) => {
        const styles = Object.fromEntries(Object.entries(expected).map(([name, properties]) => {
          const element = document.querySelector(selectors[name]);
          if (!element) throw new Error(`Missing component ${name}`);
          const computed = getComputedStyle(element);
          return [name, Object.fromEntries(Object.keys(properties).map(property => [property, computed[property]]))];
        }));
        return { styles, options: [...document.getElementById('boardSelectMobile').options]
          .map(option => ({ value: option.value, label: option.textContent, class: option.className })),
        selected: document.getElementById('boardSelectMobile').value,
        clonedIds: ['boardNavDesktopFoot', 'navbotright', 'settingsWindowLinkBot'].map(id => document.querySelectorAll(`#${id}`).length),
        footerBeforeDisclaimer: document.getElementById('boardNavDesktopFoot').nextElementSibling.id === 'absbot' };
      }, { selectors, expected: row.styles });
      expect(actual).toEqual({ styles: row.styles, options: row.options, selected: row.selected,
        clonedIds: row.clonedIds, footerBeforeDisclaimer: row.footerBeforeDisclaimer });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      if (row.width <= 480) {
        const bar = await page.locator('#boardNavMobile').boundingBox(), title = await page.locator('.boardTitle').boundingBox();
        expect(title.y).toBeGreaterThanOrEqual(bar.y + bar.height);
      }
      for (const [name, expected] of Object.entries(row.hover)) {
        const target = page.locator(selectors[name]).first(); await target.hover();
        expect(await target.evaluate((element, expected) => {
          const computed = getComputedStyle(element);
          return Object.fromEntries(Object.keys(expected).map(property => [property, computed[property]]));
        }, expected)).toEqual(expected);
        await page.mouse.move(0, 850);
      }
      expect(errors).toEqual([]); expect(external).toEqual([]);
    });
  });
}
