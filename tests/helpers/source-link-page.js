import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';

const reference = JSON.parse(await readFile(new URL('../../fixtures/source-link-page.json', import.meta.url)));
const sha = value => createHash('sha256').update(value).digest('hex');
assert.equal(sha(reference.html), reference.html_sha256);
// These already-vendored decorative assets stand in for images absent from the
// supplied tree. Their substitution is recorded; no network resource is loaded.
const assets = {};
for (const [name, path, mime] of [
  ['fade.png', 'themes/fade.png', 'image/png'],
  ['fade-blue.png', 'themes/fade-blue.png', 'image/png'],
  ['buttonfade.png', 'watcher/buttonfade.png', 'image/png'],
  ['buttonfade-blue.png', 'watcher/buttonfade-blue.png', 'image/png'],
  ['closed.gif', 'catalog/closed.gif', 'image/gif'],
]) {
  const raw = await readFile(new URL(`../../apps/public/static/${path}`, import.meta.url));
  assets[name] = `data:${mime};base64,${raw.toString('base64')}`;
}

export function sourceLinkPage(theme, worksafe = true) {
  assert.ok(Object.hasOwn(reference.themes, theme));
  let scripts = 0;
  let sheets = 0;
  let html = reference.html.replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, () => { scripts++; return ''; });
  assert.equal(scripts, 4);
  html = html.replace(/<link\b[^>]*rel="(?:alternate )?stylesheet"[^>]*>/gi, () => { sheets++; return ''; });
  assert.equal(sheets, 9);
  const css = `${reference.themes[theme].css}\n${worksafe ? reference.mobile_css : reference.mobile_nws_css}\n@media (max-width: 480px) { ${reference.dark_link_css} }`.replace(/url\(([^)]+)\)/g, (_all, value) => {
    const name = value.replace(/^['"]|['"]$/g, '').split('/').pop();
    if (assets[name]) return `url("${assets[name]}")`;
    // This closed, anonymous fixture has no pass badge or red-button element.
    assert.ok(['minileaf.gif', 'minileaf@2x.gif', 'buttonfade-red.png'].includes(name));
    return 'url("data:,")';
  });
  html = html.replace('</head>', `<style>${css}</style></head>`)
    .replaceAll('/source-assets/image/closed.gif', assets['closed.gif']);
  assert.ok(!/<script\b/i.test(html));
  return html;
}

export async function sourceLinkState(page) {
  return page.locator('#m123').evaluate(message => {
    const properties = node => {
      const style = getComputedStyle(node);
      return { color: style.color, decoration: style.textDecorationLine };
    };
    return {
      text: message.innerText.replace(/\s+/g, ' ').trim(),
      links: [...message.querySelectorAll('a')].map(node => ({
        label: node.textContent,
        href: node.getAttribute('href').replace(/^\/\/boards\.example\.test/, ''),
        ...properties(node),
      })),
      dead: [...message.querySelectorAll('.deadlink')].map(node => ({ label: node.textContent, ...properties(node) })),
    };
  });
}
