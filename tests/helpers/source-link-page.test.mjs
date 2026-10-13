import test from 'node:test';
import assert from 'node:assert/strict';
import { parse } from 'parse5';
import { sourceLinkPage } from './source-link-page.js';

function elements(node, result = []) {
  if (node.tagName) result.push(node);
  for (const child of node.childNodes ?? []) elements(child, result);
  return result;
}
for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow']) for (const worksafe of [true, false]) {
  test(`${theme} ${worksafe ? "ws" : "nws"} source page keeps the original document but cannot load scripts or external assets`, () => {
    const html = sourceLinkPage(theme, worksafe);
    const nodes = elements(parse(html, { scriptingEnabled: false }));
    const attr = (node, name) => node.attrs.find(item => item.name === name)?.value;
    assert.equal(nodes.filter(node => node.tagName === 'script').length, 0);
    assert.equal(nodes.filter(node => node.tagName === 'style').length, 2); // Original noscript + supplied styles.
    assert.equal(nodes.filter(node => node.tagName === 'link' && /stylesheet/.test(attr(node, 'rel'))).length, 0);
    for (const node of nodes.filter(node => node.tagName === 'img')) assert.match(attr(node, 'src'), /^data:image\/gif;base64,/);
    for (const [, url] of html.matchAll(/url\("([^"]+)"\)/g)) assert.match(url, /^data:/);
    assert.equal(nodes.filter(node => attr(node, 'id') === 't100').length, 1);
    assert.equal(nodes.filter(node => attr(node, 'id') === 'm123').length, 1);
    assert.equal(nodes.filter(node => attr(node, 'class') === 'deadlink').length, 3);
    assert.match(html, /#p\+123/);
    assert.match(html, /#p123e0/);
    assert.match(html, /entityneedle &amp; &lt;tag&gt;/);
    assert.match(html, /Thread closed/);
  });
}
test('source page rejects arbitrary theme keys', () => {
  for (const value of ['unknown', '__proto__', 'constructor', '../tomorrow']) assert.throws(() => sourceLinkPage(value));
});
