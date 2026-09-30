import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { mobileHeaderLabel } from '../../apps/public/client/native-post-numbers.js';

const reference = JSON.parse(await readFile(new URL('../../docs/public-tooltip-style-reference.json', import.meta.url), 'utf8'));
test.use({ javaScriptEnabled: true });
expect(reference.browser).toBe('151.0.7922.34');
expect(reference.viewport).toEqual([1280, 900]);
expect(reference.cases).toHaveLength(18);

for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow']) {
  test(`production tooltips match pinned style and edge geometry: ${theme}`, async ({ page, context }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await page.goto('/headers/');
    const text = 'Owned full <label> & text';
    await page.evaluate(({ text, short }) => {
      document.body.style.fontFamily = 'Arial';
      const header = document.getElementById('pim1001001'), name = header.querySelector('.name');
      header.style.display = 'block';
      document.querySelector('#pi1001001 .name').textContent = text;
      name.textContent = short; name.title = text;
      Object.assign(name.style, { position: 'fixed', top: '200px', width: '30px', height: '20px' });
    }, { text, short: mobileHeaderLabel(text).text });
    const label = page.locator('#pim1001001 .name'), tooltip = page.locator('#tooltip');
    for (const { left, facts } of reference.cases.filter(row => row.theme === theme)) {
      await page.mouse.move(0, 0); await expect(tooltip).toHaveCount(0);
      await label.evaluate((node, left) => { node.style.left = `${left}px`; }, left);
      await label.hover(); await expect(tooltip).toHaveText(text);
      const actual = await tooltip.evaluate(element => {
        const style = getComputedStyle(element), arrow = getComputedStyle(element, '::before');
        const properties = ['position', 'backgroundColor', 'fontFamily', 'fontSize', 'lineHeight', 'padding', 'zIndex',
          'overflowWrap', 'whiteSpace', 'maxWidth', 'color', 'textAlign'];
        const rect = element.getBoundingClientRect();
        return { className: element.className, text: element.textContent, x: rect.x, y: rect.y, width: rect.width, height: rect.height,
          style: Object.fromEntries(properties.map(key => [key, style[key]])),
          arrow: Object.fromEntries(['borderTopColor', 'borderTopWidth', 'borderLeftWidth', 'borderRightWidth', 'bottom'].map(key => [key, arrow[key]])) };
      });
      expect(actual).toEqual(facts);
    }
  });
}
