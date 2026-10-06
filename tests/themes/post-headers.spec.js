import { test, expect } from '../helpers/visual-diagnostics.js';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../docs/public-post-header-reference.json', import.meta.url), 'utf8'));
const roles = ['mod', 'admin', 'admin_highlight', 'manager', 'developer', 'founder'];
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];
const release = JSON.parse(await readFile(new URL('../../docs/public-watcher-assets.json', import.meta.url), 'utf8'));
const assets = JSON.parse(await readFile(new URL('../../docs/public-capcode-reference.json', import.meta.url), 'utf8'));
assert.equal(reference.browser, '151.0.7922.34');
assert.deepEqual(reference.viewport, [1280, 900]);
assert.equal(reference.density, 1);
assert.deepEqual(reference.client, { url: release.source, sha256: release.source_sha256, formatter_bytes: 6986 });
assert.deepEqual(reference.styles, assets.styles.map(({ url, sha256 }) => ({ url, sha256 })));
assert.deepEqual(reference.cases.map(row => `${row.theme}/${row.capcode}/${row.kind}`).sort(),
  themes.flatMap(theme => roles.flatMap(role => ['op', 'reply'].map(kind => `${theme}/${role}/${kind}`))).sort());

for (const theme of themes) {
  test(`actual OP and reply staff headers match the pinned desktop properties in ${theme}`, async ({ page, context }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await page.goto('/headers/');
    for (const row of reference.cases.filter(value => value.theme === theme)) {
      const no = String(1001001 + roles.indexOf(row.capcode) * 10 + Number(row.kind === 'reply'));
      const header = page.locator(`#pi${no}`);
      await page.mouse.move(0, 0);
      const actual = await header.evaluate(header => {
        const properties = ['fontSize', 'fontFamily', 'fontWeight', 'lineHeight', 'color'];
        const values = {};
        for (const [key, node] of [['header', header], ['name', header.querySelector('.name')],
          ['badge', header.querySelector('.capcode')], ['icon', header.querySelector('.identityIcon')],
          ['number', header.querySelector('.postNum')], ['post', header.parentElement]]) {
          const computed = getComputedStyle(node);
          values[key] = Object.fromEntries(properties.map(property => [property,
            property === 'fontFamily' ? computed[property].toLowerCase() : computed[property]]));
          if (key === 'icon') for (const property of ['width', 'height', 'marginBottom']) values[key][property] = computed[property];
          if (key === 'post') for (const property of ['padding', 'borderWidth', 'backgroundColor']) values[key][property] = computed[property];
        }
        values.labels = [...header.querySelectorAll('.postNum > a')].map((node, index) => ({ text: index ? '$post' : node.textContent, title: node.title }));
        values.links = [...header.querySelectorAll('.postNum > a')].map(node => {
          const computed = getComputedStyle(node);
          return { color: computed.color, decoration: computed.textDecorationLine };
        });
        return values;
      });
      for (let link = 0; link < 2; link++) {
        const target = header.locator('.postNum > a').nth(link);
        await target.hover();
        actual.links[link].hover = await target.evaluate(node => getComputedStyle(node).color);
      }
      expect(actual, `${theme}/${row.capcode}/${row.kind}`).toEqual(row.values);
      const thread = String(1001001 + roles.indexOf(row.capcode) * 10);
      await expect(header.getByTitle('Link to this post', { exact: true })).toHaveAttribute('href', `/demo/thread/${thread}#p${no}`);
      await expect(header.getByTitle('Reply to this post', { exact: true })).toHaveAttribute('href', `/demo/thread/${thread}?quote=${no}#reply`);
      await expect(header.locator('.postertrip,.posteruid,.flag,.bfl')).toHaveCount(0);
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  });
}

for (const density of [1, 2]) {
  test(`all staff header controls and fixed icons fit mobile at density ${density}`, async ({ browser }, info) => {
    const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width: 390, height: 844 }, deviceScaleFactor: density });
    try {
      const page = await context.newPage();
      await page.goto('http://127.0.0.1:3000/headers/');
      await expect(page.locator('.postInfo > .postNum > a')).toHaveCount(24);
      await expect(page.locator('.postInfoM > .postNum > a')).toHaveCount(24);
      const icons = page.locator('.postInfoM .identityIcon');
      await expect(icons).toHaveCount(12);
      await expect.poll(() => icons.evaluateAll(nodes => nodes.every(node => node.complete && node.naturalWidth > 0))).toBe(true);
      expect(await icons.evaluateAll(nodes => nodes.every(node => {
        const image = node.getBoundingClientRect();
        return image.width === 16 && image.height === 16 && image.left >= 0 && image.right <= innerWidth;
      }))).toBe(true);
      for (const icon of await icons.all()) {
        const src = await icon.getAttribute('src');
        const expected = density === 2 && !src.endsWith('/foundericon.gif') ? src.replace('.gif', '@2x.gif') : src;
        expect(await icon.evaluate(node => new URL(node.currentSrc).pathname)).toBe(expected);
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      const path = info.outputPath(`staff-headers-mobile-${density}.png`);
      await page.screenshot({ path, fullPage: true });
      await info.attach(`Owned mobile staff headers at density ${density}`, { path, contentType: 'image/png' });
    } finally { await context.close(); }
  });
}
