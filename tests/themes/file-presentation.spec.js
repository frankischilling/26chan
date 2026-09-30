import { test, expect } from '@playwright/test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-file-style-reference.json', import.meta.url)));
assert.equal(reference.browser, '151.0.7922.34');
assert.equal(reference.density, 1);
assert.equal(reference.cases.length, 24);
for (const row of reference.cases) {
  test(`released file properties: ${row.family}/${row.theme}/${row.viewport[0]}`, async ({ page, context }) => {
    await page.setViewportSize({ width: row.viewport[0], height: row.viewport[1] });
    await context.addCookies([{ name: 'board-theme-ws', value: row.theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await page.goto('/img/thread/1000201');
    await page.evaluate(family => document.body.dataset.worksafe = String(family === 'yotsubluemobile'), row.family);
    const actual = await page.locator('#f1000201').evaluate(file => {
      const facts = {};
      for (const [key, selector, properties] of [
        ['header', '.fileText', ['display', 'whiteSpace', 'maxWidth']],
        ['thumb', '.fileThumb', ['float', 'margin', 'textDecorationLine']],
        ['image', '.fileThumb img', ['float', 'maxWidth', 'maxHeight', 'objectFit', 'width', 'height']],
        ['caption', '.mFileInfo', ['display', 'paddingTop', 'textAlign', 'color', 'fontSize', 'textDecorationLine']],
      ]) {
        const computed = getComputedStyle(file.querySelector(selector));
        facts[key] = Object.fromEntries(properties.map(property => [property, computed[property]]));
      }
      return facts;
    });
    expect(actual).toEqual(row.values);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(row.viewport[0]);
  });
}
