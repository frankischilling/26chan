import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { posterIdColor } from '../../apps/public/static/native-display.v1.js';

test('ID colors retain the released client hash and finite alphabet', () => {
  // Fixed vectors independently calculated as sum(codepoint * 31**position).
  assert.deepEqual(posterIdColor('AAAAAAAA'), { background: 'rgb(65, 62, 240)', color: 'white' });
  assert.deepEqual(posterIdColor('12345678'), { background: 'rgb(145, 14, 0)', color: 'white' });
  assert.deepEqual(posterIdColor('Heaven'), { background: 'rgb(128, 154, 18)', color: 'black' });
  for (const id of [null, {}, 12345678, '', '1234567', '123456789', '1234567\n', '12345678\n', '1234567<', 'é2345678', 'heaven', 'Heaven\n', 'Heaven ', 'Mod', 'Admin']) {
    assert.equal(posterIdColor(id), null);
  }
});

test('actual browser colors survive CSP, settings, live labels, preview insertion and BFCache', async () => {
  const origin = 'https://poster-ids.example';
  const source = await readFile(new URL('../../apps/public/static/native-display.v1.js', import.meta.url), 'utf8');
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    const unexpected = [];
    await page.route('**/*', async route => {
      const url = new URL(route.request().url());
      if (url.href === `${origin}/static/native-display.v1.js`) return route.fulfill({ contentType: 'text/javascript', body: source });
      if (url.href === `${origin}/test/`) return route.fulfill({ contentType: 'text/html',
        headers: { 'content-security-policy': `default-src 'none'; script-src ${origin}/static/native-display.v1.js; style-src 'self'; base-uri 'none'` },
        body: '<!doctype html><html><head><meta charset="utf-8"></head><body><main id="owned"><span class="posteruid">(ID: <span class="hand" id="first">AAAAAAAA</span>)</span></main></body></html>' });
      unexpected.push(url.href); await route.abort();
    });
    await page.goto(`${origin}/test/`);
    await page.evaluate(async () => {
      window.api = await import('/static/native-display.v1.js'); window.config = {};
      window.root = document.getElementById('owned');
      window.controller = api.mountNativePosterIds({ root, settings: () => config });
    });
    assert.equal(await page.locator('#first').evaluate(element => getComputedStyle(element).backgroundColor), 'rgb(65, 62, 240)');
    await page.evaluate(() => {
      document.getElementById('first').textContent = '12345678';
      const label = document.createElement('span'); label.className = 'posteruid';
      const text = document.createElement('span'); text.className = 'hand'; text.id = 'preview'; text.textContent = 'Heaven';
      label.append(text); root.append(label);
    });
    assert.equal(await page.locator('#first').evaluate(element => element.style.backgroundColor), 'rgb(145, 14, 0)');
    assert.equal(await page.locator('#preview').evaluate(element => element.style.backgroundColor), 'rgb(128, 154, 18)');
    assert.equal(await page.locator('#preview').evaluate(element => element.style.color), 'black');
    await page.evaluate(() => { config.IDColor = false; document.dispatchEvent(new Event('4chanSettingsSaved')); });
    assert.equal(await page.locator('#first').evaluate(element => element.style.backgroundColor), '');
    await page.evaluate(() => { config.IDColor = true; document.dispatchEvent(new Event('4chanPreferencesRestored')); });
    assert.equal(await page.locator('#first').evaluate(element => element.style.backgroundColor), 'rgb(145, 14, 0)');
    await page.evaluate(() => {
      document.getElementById('first').style.color = 'red';
      config.disableAll = true; controller.refresh();
    });
    assert.equal(await page.locator('#first').evaluate(element => element.style.color), 'red');
    assert.equal(await page.locator('#first').evaluate(element => element.style.backgroundColor), '');
    await page.evaluate(() => {
      config.disableAll = false; controller.refresh();
      window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
      document.getElementById('first').textContent = 'AAAAAAAA';
    });
    assert.equal(await page.locator('#first').evaluate(element => element.style.backgroundColor), 'rgb(145, 14, 0)');
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
    assert.equal(await page.locator('#first').evaluate(element => element.style.backgroundColor), 'rgb(65, 62, 240)');
    await page.evaluate(() => root.remove());
    assert.equal(await page.evaluate(() => root.querySelector('#first').style.backgroundColor), '');
    await page.evaluate(() => { document.body.append(root); controller.refresh(); });
    assert.equal(await page.locator('#first').evaluate(element => element.style.backgroundColor), '');
    assert.deepEqual(unexpected, []);
  } finally { await browser.close(); }
});
