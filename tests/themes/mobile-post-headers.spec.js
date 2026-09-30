import { test, expect } from '@playwright/test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const json = async path => JSON.parse(await readFile(new URL(path, import.meta.url), 'utf8'));
const reference = await json('../../docs/public-mobile-header-reference.json');
const release = await json('../../docs/public-watcher-assets.json');
const badges = await json('../../docs/public-capcode-reference.json');
const navigation = await json('../../docs/public-watcher-navigation-reference.json');
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];
const roles = ['mod', 'admin', 'admin_highlight', 'manager', 'developer', 'founder'];
const families = ['yotsubamobile', 'yotsubluemobile'];
assert.equal(reference.browser, '151.0.7922.34');
assert.deepEqual(reference.viewport, [390, 844]);
assert.equal(reference.density, 1);
assert.deepEqual(reference.client, { url: release.source, sha256: release.source_sha256, formatter_bytes: 6986 });
assert.deepEqual(reference.extensionCss, { bytes: 21407, sha256: '4e79f90a330a7b733c35623b227827b92164ee3c44f79b918807c0f3876ca717' });
assert.deepEqual(reference.styles, [...badges.styles.map(({ url, sha256 }) => ({ url, sha256 })),
  ...families.map(name => { const { url, sha256 } = navigation.stylesheets.find(row => row.url.endsWith(`/${name}.716.css`)); return { url, sha256 }; })]);
assert.deepEqual(reference.cases.map(row => `${row.mobile}/${row.dark}/${row.theme}/${row.capcode}/${row.kind}`).sort(),
  families.flatMap(mobile => [false, true].flatMap(dark => themes.flatMap(theme => roles.flatMap(role =>
    ['op', 'reply'].map(kind => `${mobile}/${dark}/${theme}/${role}/${kind}`))))).sort());

for (const mobile of families) for (const dark of [false, true]) for (const theme of themes) {
  test(`production mobile staff headers match released properties: ${mobile}/${dark ? 'dark' : 'ordinary'}/${theme}`, async ({ page, context }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await context.addCookies([{ name: mobile === 'yotsubluemobile' ? 'board-theme-ws' : 'board-theme',
      value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await page.goto(mobile === 'yotsubluemobile' ? '/headers/' : '/headers-nws/');
    // These facts qualify static CSS classes. Native preference transitions are
    // covered separately on persisted pages with scripts enabled.
    await page.evaluate(dark => document.body.classList.toggle('m-dark', dark), dark);
    for (const row of reference.cases.filter(row => row.mobile === mobile && row.dark === dark && row.theme === theme)) {
      const no = String(1001001 + roles.indexOf(row.capcode) * 10 + Number(row.kind === 'reply'));
      const header = page.locator(`#pim${no}`);
      await page.mouse.move(0, 0);
      const actual = await header.evaluate(header => {
        const properties = ['display', 'float', 'clear', 'padding', 'margin', 'borderWidth', 'borderColor',
          'backgroundColor', 'fontSize', 'fontFamily', 'fontWeight', 'lineHeight', 'color', 'textAlign'];
        const values = {};
        for (const [key, node] of [['header', header], ['nameBlock', header.querySelector('.nameBlock')],
          ['name', header.querySelector('.name')], ['subject', header.querySelector('.subject')],
          ['badge', header.querySelector('.capcode')], ['icon', header.querySelector('.identityIcon')],
          ['numberDate', header.querySelector('.postNum')], ['post', header.parentElement]]) {
          if (!node) { values[key] = null; continue; }
          const computed = getComputedStyle(node);
          values[key] = Object.fromEntries(properties.map(property => [property,
            property === 'fontFamily' ? computed[property].toLowerCase() : computed[property]]));
          if (key === 'icon') for (const property of ['width', 'height']) values[key][property] = computed[property];
        }
        values.desktopDisplay = getComputedStyle(header.parentElement.querySelector('.postInfo')).display;
        values.links = [...header.querySelectorAll('a')].map((node, index) => ({ text: index ? '$post' : node.textContent,
          title: node.title, color: getComputedStyle(node).color }));
        values.classes = header.className;
        values.order = [...header.children].map(node => ({ tag: node.localName, class: node.className }));
        return values;
      });
      for (let link = 0; link < 2; link++) {
        const target = header.locator('.postNum > a').nth(link);
        await target.hover(); actual.links[link].hover = await target.evaluate(node => getComputedStyle(node).color);
      }
      expect.soft(actual, `${mobile}/${dark}/${theme}/${row.capcode}/${row.kind}`).toEqual(row.values);
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  });
}
