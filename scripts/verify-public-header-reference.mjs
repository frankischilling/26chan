import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { chromium } from '@playwright/test';

const root = new URL('../', import.meta.url), args = process.argv.slice(2);
assert.ok(args.length === 2 || (args.length === 3 && args[2] === '--write'),
  'Use <extension.1191.js> <desktop-css-directory> [--write]');
const json = async path => JSON.parse(await readFile(new URL(path, root), 'utf8'));
const release = await json('docs/public-watcher-assets.json');
const assets = await json('docs/public-capcode-reference.json');
const digest = value => createHash('sha256').update(value).digest('hex');
const source = await readFile(resolve(args[0]));
assert.equal(digest(source), release.source_sha256, 'Released client differs from its pin');
const text = source.toString('utf8'), marker = 'Parser.buildHTMLFromJSON=';
assert.equal(text.split(marker).length, 2);
const start = text.indexOf(marker) + marker.length, end = text.indexOf(',Parser.truncate=function', start);
assert.ok(end > start && end - start === 6986, 'Inspected formatter boundaries changed');
const formatter = text.slice(start, end);
const roles = ['mod', 'admin', 'admin_highlight', 'manager', 'developer', 'founder'];
const styles = ['yotsubanew', 'yotsubluenew', 'futabanew', 'burichannew', 'photon', 'tomorrow'];
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];
const images = new Map();
for (const asset of assets.assets) {
  const value = await readFile(new URL(`apps/public/static/identity/${asset.name}`, root));
  assert.equal(value.length, asset.bytes); assert.equal(digest(value), asset.sha256);
  images.set(`/image/${asset.name}`, value);
}
const browser = await chromium.launch({ headless: true });
try {
  assert.equal(browser.version(), '151.0.7922.34', 'Reference browser differs from its pin');
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 });
  const unexpected = [];
  await context.route('**/*', async route => {
    const url = new URL(route.request().url());
    if (url.origin === 'https://reference.invalid' && images.has(url.pathname)) {
      await route.fulfill({ contentType: 'image/gif', body: images.get(url.pathname) }); return;
    }
    unexpected.push(url.origin + url.pathname); await route.abort();
  });
  const page = await context.newPage(), cases = [];
  for (const [index, style] of styles.entries()) {
    const css = await readFile(new URL(`${style}.716.css`, pathToFileURL(resolve(args[1]) + '/')));
    const pin = assets.styles.find(value => value.url.endsWith(`/${style}.716.css`));
    assert.ok(pin); assert.equal(digest(css), pin.sha256);
    for (const capcode of roles) for (const kind of ['op', 'reply']) {
      await page.mouse.move(0, 0);
      await page.setContent(`<!doctype html><html><head><style>${css.toString('utf8')}</style></head><body><main class="board"><section class="thread"></section></main></body></html>`);
      const values = await page.evaluate(({ formatter, capcode, kind }) => {
        // Execute only the inspected pure formatter branch with fixed synthetic
        // fields. No extension initialization, public post or server is loaded.
        window.Main = { board: 'demo', tid: 0, hasMobileLayout: false };
        window.Config = { revealSpoilers: false };
        window.$L = { d: () => 'reference.invalid' };
        window.Parser = { icons: Object.fromEntries([
          ['mod', 'modicon'], ['admin', 'adminicon'], ['dev', 'developericon'],
          ['manager', 'managericon'], ['founder', 'foundericon'],
        ].map(([key, value]) => [key, `https://reference.invalid/image/${value}.gif`])) };
        const build = (0, eval)(`(${formatter})`);
        const post = build({ no: kind === 'op' ? 1001001 : 1001002, resto: kind === 'op' ? 0 : 1001001,
          name: 'Owned staff', sub: 'Owned header subject', capcode, time: 1788868800,
          now: '09/08/26(Tue)08:00:00', com: 'Owned synthetic header text' }, 'demo', kind === 'op', false);
        // Public Depager passes true for an OP and omits it for replies.
        if (!post.querySelector('.post').classList.contains(kind)) throw new Error('reference post kind differs');
        document.querySelector('.thread').append(post);
        const header = post.querySelector('.postInfo.desktop');
        const properties = ['fontSize', 'fontFamily', 'fontWeight', 'lineHeight', 'color'];
        const values = {};
        for (const [key, node] of [['header', header], ['name', header.querySelector('.name')],
          ['badge', header.querySelector('.capcode')], ['icon', header.querySelector('.identityIcon')],
          ['number', header.querySelector('.postNum')], ['post', post.querySelector('.post')]]) {
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
      }, { formatter, capcode, kind });
      for (let link = 0; link < 2; link++) {
        const target = page.locator('.postInfo.desktop .postNum > a').nth(link);
        await target.hover();
        values.links[link].hover = await target.evaluate(node => getComputedStyle(node).color);
      }
      cases.push({ theme: themes[index], capcode, kind, values });
    }
  }
  assert.deepEqual(unexpected, [], 'Reference attempted an unapproved resource');
  const result = {
    scope: 'Computed desktop header and staff-badge properties from the inspected released formatter, six pinned desktop styles, and synthetic OP/reply inputs. This does not qualify full-page pixels, mobile headers, private staff authorization or original-server formatting.',
    browser: browser.version(), viewport: [1280, 900], density: 1,
    client: { url: release.source, sha256: release.source_sha256, formatter_bytes: 6986 },
    styles: assets.styles.map(({ url, sha256 }) => ({ url, sha256 })),
    cases,
  };
  const target = new URL('docs/public-post-header-reference.json', root);
  if (args[2] === '--write') await writeFile(target, JSON.stringify(result, null, 2) + '\n');
  else assert.deepEqual(result, await json('docs/public-post-header-reference.json'), 'Recorded header facts differ');
  await context.close();
  console.log(`Verified ${cases.length} pinned desktop OP/reply header cases with no external requests.`);
} finally { await browser.close(); }
