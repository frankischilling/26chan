import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

test('finite link DOM preserves source text, nodes, soft breaks and anchor boundaries', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.route('**/*', route => route.abort());
    await page.goto('about:blank');
    const code = await readFile(new URL('../../apps/public/client/native-linkification.js', import.meta.url), 'utf8');
    await page.evaluate(async source => {
      const module = await import(URL.createObjectURL(new Blob([source], { type: 'text/javascript' })));
      window.linkify = module.linkifyMessage;
    }, code);
    const results = await page.evaluate(() => {
      const cases = [
        ['https://example.test/path?!', ['https://example.test/path']],
        ['(https://example.test/a(b)).', ['https://example.test/a(b))']],
        ['https://example.test/abcd<wbr>efgh?a=1&amp;b=2', ['https://example.test/abcdefgh?a=1&b=2']],
        ['<s>https://example.test/path</s>', ['https://example.test/path']],
        ['HTTPS://UPPER.TEST/path', []],
        ['<a href="https://lower.test">existing</a> HTTPS://UPPER.TEST/path', []],
        ['<a href="https://lower.test">https://lower.test</a> HTTPS://UPPER.TEST/path', ['HTTPS://UPPER.TEST/path']],
        ['https://example.test:8443/path', ['https://example.test']],
        ['https://one.test https://two.test https://three.test', ['https://one.test', 'https://two.test', 'https://three.test']],
        ['https://example.test/ab<span>cd</span>', ['https://example.test/ab']],
        ['Bhttps://example.test/path "https://other.test/path"', []],
        ['😀 &amp; https://example.test/a&lt;b https://next.test/', ['https://example.test/a<b', 'https://next.test/']],
        ['https://example.test/ab\u200bcd x\u200by', ['https://example.test/abcd']],
      ];
      return cases.map(([html, expected]) => {
        const message = document.createElement('blockquote');
        // These are fixed synthetic fixtures, not application rendering.
        message.innerHTML = html; document.body.replaceChildren(message);
        const before = message.textContent.replaceAll('\u200b', '');
        const existing = message.querySelector('a'), spoiler = message.querySelector('s');
        const count = window.linkify(message), after = message.textContent;
        const anchors = [...message.querySelectorAll('a.linkified')];
        const result = { html, count, expected, labels: anchors.map(a => a.textContent), before, after,
          attributes: anchors.map(a => [a.getAttribute('href'), a.target, a.rel]),
          keptExisting: !existing || existing === message.querySelector('a'),
          keptSpoiler: !spoiler || spoiler === message.querySelector('s'),
          nested: message.querySelectorAll('a a').length,
          breaks: message.querySelectorAll('wbr').length };
        result.second = window.linkify(message);
        return result;
      });
    });
    for (const result of results) {
      assert.deepEqual(result.labels, result.expected, result.html);
      assert.equal(result.count, result.expected.length, result.html);
      assert.equal(result.after, result.before, result.html);
      assert.equal(result.second, 0, result.html);
      assert.ok(result.keptExisting && result.keptSpoiler, result.html);
      assert.equal(result.nested, 0, result.html);
      for (const [href, target, rel] of result.attributes) {
        assert.ok(href.startsWith('/derefer?url='));
        assert.equal(target, '_blank'); assert.equal(rel, 'noreferrer nofollow noopener');
      }
      if (result.html.includes('<wbr>')) assert.equal(result.breaks, 1);
      if (result.html.includes('\u200b')) assert.equal(result.breaks, 2);
    }
    const rejected = await page.evaluate(() => {
      return ['<svg></svg> https://example.test', '<!-- comment -->https://example.test',
        '<span>'.repeat(34) + 'https://example.test' + '</span>'.repeat(34),
        'https://example.test/' + 'x'.repeat(192000),
        '<a href="' + 'x'.repeat(192000) + '">old</a> https://example.test'].map(html => {
        const message = document.createElement('blockquote'); message.innerHTML = html;
        const before = message.innerHTML;
        return [window.linkify(message), message.innerHTML === before];
      });
    });
    assert.deepEqual(rejected, Array(5).fill([0, true]));
  } finally { await browser.close(); }
});
