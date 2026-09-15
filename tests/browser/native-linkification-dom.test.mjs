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
      window.mountLinkification = module.mountNativeLinkification;
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
        assert.equal(target, '_blank'); assert.equal(rel, 'noreferrer ugc noopener');
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

test('mounted linkification follows settings, mobile defaults and live post insertion without replacing server anchors', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.route('**/*', route => route.abort());
    await page.goto('about:blank');
    const code = await readFile(new URL('../../apps/public/client/native-linkification.js', import.meta.url), 'utf8');
    const result = await page.evaluate(async source => {
      const module = await import(URL.createObjectURL(new Blob([source], { type: 'text/javascript' })));
      document.body.innerHTML = '<main class="board"><article><blockquote class="postMessage" id="m1">'
        + 'https://initial.test/path <a id="server" class="linkified" href="https://historical.test/" rel="nofollow noreferrer noopener">historical</a>'
        + '</blockquote></article></main>';
      const root = document.querySelector('.board');
      const server = document.getElementById('server');
      let settings = {}, neverMobile = null, unavailable = false, settingsUnavailable = false;
      const listeners = new Set();
      const mobile = {
        matches: false,
        addEventListener(type, listener) { if (type === 'change') listeners.add(listener); },
        removeEventListener(type, listener) { if (type === 'change') listeners.delete(listener); },
        set(value) { this.matches = value; for (const listener of listeners) listener({ matches: value }); },
      };
      const mounted = module.mountNativeLinkification({ root, settings: () => {
        if (settingsUnavailable) throw new DOMException('Unavailable', 'SecurityError');
        return settings;
      }, mobile,
        readNeverMobile: () => { if (unavailable) throw new DOMException('Unavailable', 'SecurityError'); return neverMobile; } });
      const flush = () => new Promise(resolve => setTimeout(resolve, 0));
      const count = selector => document.querySelectorAll(selector).length;
      const output = { desktopDefault: count('#m1 a[data-native-linkified]') };

      settings = { linkify: true };
      document.dispatchEvent(new Event('4chanSettingsSaved'));
      output.desktopOptIn = count('#m1 a[data-native-linkified]');
      output.serverAfterEnable = server === document.getElementById('server') && server.isConnected;

      settings = { linkify: true, disableAll: true };
      document.dispatchEvent(new Event('4chanSettingsSaved'));
      output.disabled = count('#m1 a[data-native-linkified]');
      output.serverAfterDisable = server === document.getElementById('server') && server.isConnected;

      settings = { linkify: false };
      mobile.set(true);
      output.mobileDefault = count('#m1 a[data-native-linkified]');
      neverMobile = 'true'; mounted.refresh();
      output.neverMobile = count('#m1 a[data-native-linkified]');
      neverMobile = 'false'; mounted.refresh();
      output.otherStorageValue = count('#m1 a[data-native-linkified]');
      unavailable = true; mounted.refresh();
      output.unavailableStorage = count('#m1 a[data-native-linkified]');
      unavailable = false; settingsUnavailable = true; mounted.refresh();
      output.unavailableSettingsMobile = count('#m1 a[data-native-linkified]');
      mobile.set(false);
      output.unavailableSettingsDesktop = count('#m1 a[data-native-linkified]');
      settingsUnavailable = false; mobile.set(true);

      const reply = document.createElement('article');
      reply.innerHTML = '<blockquote class="postMessage" id="m2">https://reply.test/new</blockquote>';
      root.append(reply);
      const preview = document.createElement('article');
      preview.id = 'quote-preview'; preview.className = 'post preview';
      preview.innerHTML = '<blockquote class="postMessage" id="preview-message">https://preview.test/new</blockquote>';
      document.body.append(preview);
      await flush();
      output.dynamicReply = count('#m2 a[data-native-linkified]');
      output.quotePreview = count('#preview-message a[data-native-linkified]');

      settings = { linkify: true, disableAll: true };
      document.dispatchEvent(new Event('4chanSettingsSaved'));
      output.generatedAfterDisable = count('a[data-native-linkified]');
      output.serverFinal = server === document.getElementById('server') && server.isConnected
        && server.getAttribute('href') === 'https://historical.test/';
      settings = { linkify: true };
      window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
      window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
      const restored = document.createElement('blockquote');
      restored.id = 'restored-message'; restored.className = 'postMessage'; restored.textContent = 'https://restored.test/path';
      root.append(restored); await flush();
      output.afterCacheRestore = count('#restored-message a[data-native-linkified]');
      window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: false }));
      const retired = document.createElement('blockquote');
      retired.id = 'retired-message'; retired.className = 'postMessage'; retired.textContent = 'https://retired.test/path';
      root.append(retired); await flush();
      output.afterPagehide = count('#retired-message a[data-native-linkified]');
      mounted.disconnect();
      return output;
    }, code);
    assert.deepEqual(result, {
      desktopDefault: 0,
      desktopOptIn: 1,
      serverAfterEnable: true,
      disabled: 0,
      serverAfterDisable: true,
      mobileDefault: 1,
      neverMobile: 0,
      otherStorageValue: 1,
      unavailableStorage: 1,
      unavailableSettingsMobile: 1,
      unavailableSettingsDesktop: 0,
      dynamicReply: 1,
      quotePreview: 1,
      generatedAfterDisable: 0,
      serverFinal: true,
      afterCacheRestore: 1,
      afterPagehide: 0,
    });
  } finally { await browser.close(); }
});
