import { test, expect } from '../helpers/visual-diagnostics.js';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-page-dark-reference.json', import.meta.url)));
const selectors = { page: 'body', banner: '.boardBanner', desktop: '#boardNavDesktop',
  desktopLink: '#boardNavDesktop .boardList a', mobile: '#boardNavMobile', mobileSelect: '#boardSelectMobile',
  mobileLink: '#settingsWindowLinkMobile', title: '.boardTitle', subtitle: '.boardSubtitle', footer: '#boardNavDesktopFoot',
  footerLink: '#navbotright a', footerLinks: '#footer-links', footerText: '#absbot' };

for (const row of reference.cases) {
  test.describe(`${row.theme} mobile dark worksafe=${row.worksafe} ${row.width} DPR ${row.scale}`, () => {
    test.use({ viewport: { width: row.width, height: 900 }, deviceScaleFactor: row.scale });
    test('navigation and title retain the released dark colors', async ({ page, context }) => {
      await context.addCookies([{ name: row.worksafe ? 'board-theme-ws' : 'board-theme', value: row.theme, url: 'http://127.0.0.1:3000' }]);
      await page.goto(`/chrome/${row.worksafe ? 'demo' : 'zed'}/`);
      // Static CSS class facts; persisted dark-mode transitions have their own
      // native-layout application tests.
      await page.evaluate(() => document.body.classList.add('m-dark'));
      const actual = await page.evaluate(({ selectors, values }) => Object.fromEntries(Object.entries(values).map(([name, properties]) => {
        const computed = getComputedStyle(document.querySelector(selectors[name]));
        return [name, Object.fromEntries(Object.keys(properties).map(property => [property, computed[property]]))];
      })), { selectors, values: row.values });
      expect(actual).toEqual(row.values);
      for (const [name, expected] of Object.entries(row.hover)) {
        const target = page.locator(name === 'footerLinks' ? '#footer-links a' : selectors[name]).first();
        await target.hover();
        expect(await target.evaluate(node => ({ color: getComputedStyle(node).color,
          borderBottomColor: getComputedStyle(node).borderBottomColor }))).toEqual(expected);
        await page.mouse.move(0, 850);
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    });
  });
}
