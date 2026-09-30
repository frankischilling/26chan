import { test, expect } from '@playwright/test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-file-states-reference.json', import.meta.url)));
assert.equal(reference.browser, '151.0.7922.34');
assert.equal(reference.states.length, 20);
assert.equal(reference.cases.length, 192);
for (const density of [1, 2]) {
  test.describe(`file states at ${density}x`, () => {
    test.use({ deviceScaleFactor: density });
    for (const row of reference.cases.filter(row => row.density === density)) {
      test(`${row.family}/${row.theme}/${row.viewport[0]}/${row.kind}/${row.state}`, async ({ page, context }) => {
        await page.setViewportSize({ width: row.viewport[0], height: row.viewport[1] });
        await context.addCookies([{ name: 'board-theme-ws', value: row.theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
        await page.goto(row.kind === 'op' ? '/img/file-states' : '/img/thread/1000201');
        await page.evaluate(family => document.body.dataset.worksafe = String(family === 'yotsubluemobile'), row.family);
        const file = page.locator(`#f${row.state === 'spoiler' ? '1000205' : '1000206'}`);
        const image = file.locator('img'); await image.scrollIntoViewIfNeeded();
        await expect.poll(() => image.evaluate(node => node.complete && node.naturalWidth > 0)).toBe(true);
        if (row.state === 'deleted') {
          expect(await image.evaluate(node => new URL(node.currentSrc).pathname)).toBe(`/static/catalog/filedeleted-res${density === 2 ? '@2x' : ''}.gif`);
        }
        const actual = await file.evaluate(file => {
          const values = {};
          for (const [key, selector, properties] of [
            ['thumb', '.fileThumb', ['float', 'margin', 'textDecorationLine']],
            ['image', '.fileThumb img', ['float', 'width', 'height', 'maxWidth', 'maxHeight', 'objectFit']],
          ]) {
            const computed = getComputedStyle(file.querySelector(selector));
            values[key] = Object.fromEntries(properties.map(property => [property, computed[property]]));
          }
          return values;
        });
        expect(actual).toEqual(row.values);
        expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(row.viewport[0]);
      });
    }
  });
}
