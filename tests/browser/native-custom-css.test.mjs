import test, { after, before } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { CUSTOM_CSS_LIMITS, parseCustomCSS } from '../../apps/public/static/native-custom-css.v1.js';

test('parser accepts only bounded post selectors and finite presentation values', () => {
  const result = parseCustomCSS(`
    .reply, div.op {
      color: #AABBCC;
      background-color: #123;
      font-family: Arial, Helvetica, sans-serif;
      font-size: 16px;
      font-weight: 700;
      line-height: 1.4;
      margin: 4px 8px;
      padding-left: 12px;
    }
    .postMessage { letter-spacing: 0.5px; text-align: left; }
  `);
  assert.equal(result.status, 'ok');
  assert.match(result.css, /\.board \.post\.reply, \.board \.post\.op/);
  assert.match(result.css, /color: #aabbcc/);
  assert.match(result.css, /font-family: Arial, Helvetica, sans-serif/);
  assert.equal(result.rules.length, 2);
  assert.ok(result.bytes < CUSTOM_CSS_LIMITS.bytes);
  assert.deepEqual(parseCustomCSS(''), { status: 'ok', bytes: 0, rules: [], css: '' });
});

test('parser rejects escapes, external loads, variables, arbitrary selectors and UI-hiding syntax all-or-nothing', () => {
  const invalid = [
    '.po\\73 t { color: #fff; }',
    '@import "https://example.invalid/a.css";',
    '.reply { background-color: url(https://example.invalid/a.png); }',
    '.reply { color: var(--ink); }',
    '.reply { --ink: #fff; color: #000; }',
    '.reply { display: none; }',
    '.reply { position: fixed; }',
    '.reply { content: "hidden"; }',
    'body { color: #fff; }',
    '#settingsMenu { color: #fff; }',
    '.reply:hover { color: #fff; }',
    '.reply { color: inherit; }',
    '.reply { color: #fff !important; }',
    '.reply { margin-top: 4px 8px; }',
    '.reply { padding-left: 4px 8px; }',
    '.reply { color: #fff; } /* hidden */',
    '.reply { color: #fff; } .postMessage { color: #000; display: none; }',
    '.reply { color: #fff; .postMessage { color: #000; } }',
  ];
  for (const source of invalid) {
    const result = parseCustomCSS(source);
    assert.equal(result.status, 'invalid', source);
    assert.ok(result.error.length > 0, source);
  }
  assert.equal(parseCustomCSS('.reply { color: #fff; }'.repeat(CUSTOM_CSS_LIMITS.rules + 1)).status, 'invalid');
  assert.equal(parseCustomCSS('x'.repeat(CUSTOM_CSS_LIMITS.bytes + 1)).status, 'invalid');
  assert.equal(parseCustomCSS(`.reply { color: #fff; }${' '.repeat(CUSTOM_CSS_LIMITS.bytes)}x`).status, 'invalid');
});

const source = await readFile(new URL('../../apps/public/static/native-custom-css.v1.js', import.meta.url), 'utf8');
const origin = 'https://custom-css.example';
let browser;

before(async () => { browser = await chromium.launch({ headless: true }); });
after(async () => { await browser?.close(); });

async function fixture(t, { css = '.reply { background-color: #112233; padding-left: 8px; }', settings = { customCSS: true } } = {}) {
  const context = await browser.newContext({ viewport: { width: 1000, height: 700 } });
  t.after(() => context.close());
  const requests = [];
  await context.route('**/*', async route => {
    const url = new URL(route.request().url());
    if (url.origin === origin && url.pathname === '/static/native-custom-css.v1.js') {
      return route.fulfill({ contentType: 'text/javascript', body: source });
    }
    if (route.request().isNavigationRequest()) {
      return route.fulfill({
        contentType: 'text/html',
        headers: { 'content-security-policy': "default-src 'none'; script-src 'self'; style-src 'self'" },
        body: '<!doctype html><html><head><meta charset="utf-8"></head><body>'
          + '<button id="opener" type="button">Open</button><main class="board">'
          + '<article class="postContainer"><div class="post op" id="p100"><div class="postInfo">'
          + '<span class="subject">Subject</span> <span class="name">Anonymous</span></div>'
          + '<blockquote class="postMessage">OP <a class="quotelink" href="#p101">&gt;&gt;101</a></blockquote></div></article>'
          + '<article class="postContainer"><div class="post reply" id="p101"><div class="postInfo">'
          + '<span class="name">Anonymous</span></div><blockquote class="postMessage">Reply</blockquote></div></article>'
          + '</main></body></html>',
      });
    }
    requests.push(url.href);
    return route.abort();
  });
  const page = await context.newPage();
  await page.goto(`${origin}/demo/thread/100`);
  await page.evaluate(async ({ css, settings }) => {
    window.customCSSAPI = await import('/static/native-custom-css.v1.js');
    window.cssValue = css;
    window.settingsState = settings;
    window.saveMode = 'immediate';
    window.saveCalls = [];
    window.saveCSS = (raw, expected, signal) => new Promise(resolve => {
      const call = { raw, expected, aborted: false };
      window.saveCalls.push(call);
      const abort = () => { call.aborted = true; resolve({ status: 'conflict' }); };
      signal.addEventListener('abort', abort, { once: true });
      const finish = () => {
        if (signal.aborted) return;
        signal.removeEventListener('abort', abort);
        if (window.cssValue !== expected) { resolve({ status: 'conflict' }); return; }
        window.cssValue = raw === '' ? null : raw;
        resolve({ status: 'ok', persisted: true });
      };
      if (window.saveMode === 'delayed') window.releaseCustomSave = finish;
      else finish();
    });
    window.mountCustomCSS = () => customCSSAPI.mountNativeCustomCSS({
      root: document.querySelector('.board'),
      settings: () => window.settingsState,
      readCSS: () => window.cssValue,
      saveCSS: window.saveCSS,
    });
    window.customCSS = window.mountCustomCSS();
  }, { css, settings });
  return { context, page, requests };
}

const replyStyle = page => page.locator('#p101').evaluate(element => ({
  background: getComputedStyle(element).backgroundColor,
  paddingLeft: getComputedStyle(element).paddingLeft,
}));

test('constructed stylesheet follows settings, BFCache lifecycle, duplicate mounts and owned teardown', async t => {
  const { page, requests } = await fixture(t);
  assert.deepEqual(await replyStyle(page), { background: 'rgb(17, 34, 51)', paddingLeft: '8px' });
  assert.deepEqual(requests, []);
  assert.equal(await page.locator('style').count(), 0);
  assert.equal(await page.evaluate(() => document.adoptedStyleSheets.filter(sheet => [...sheet.cssRules]
    .some(rule => rule.cssText.includes('.board .post.reply'))).length), 1);

  await page.evaluate(() => {
    settingsState = { customCSS: false };
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.deepEqual(await replyStyle(page), { background: 'rgba(0, 0, 0, 0)', paddingLeft: '0px' });
  await page.evaluate(() => {
    settingsState = { customCSS: true };
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
  });
  assert.deepEqual(await replyStyle(page), { background: 'rgb(17, 34, 51)', paddingLeft: '8px' });

  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  assert.deepEqual(await replyStyle(page), { background: 'rgba(0, 0, 0, 0)', paddingLeft: '0px' });
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  assert.deepEqual(await replyStyle(page), { background: 'rgb(17, 34, 51)', paddingLeft: '8px' });

  await page.evaluate(() => { window.previousCustomCSS = customCSS; customCSS = window.mountCustomCSS(); });
  assert.equal(await page.evaluate(() => document.adoptedStyleSheets.filter(sheet => [...sheet.cssRules]
    .some(rule => rule.cssText.includes('.board .post.reply'))).length), 1);
  await page.evaluate(() => previousCustomCSS.refresh());
  assert.equal(await page.evaluate(() => document.adoptedStyleSheets.filter(sheet => [...sheet.cssRules]
    .some(rule => rule.cssText.includes('.board .post.reply'))).length), 1);

  await page.evaluate(() => customCSS.destroy());
  assert.deepEqual(await replyStyle(page), { background: 'rgba(0, 0, 0, 0)', paddingLeft: '0px' });
});

test('editor cancels delayed saves on close, enabled-to-disabled transition and pagehide', async t => {
  const { page } = await fixture(t);

  async function beginSave(value) {
    await page.evaluate(() => { saveMode = 'delayed'; customCSS.open(document.getElementById('opener')); });
    await page.getByLabel('Post CSS', { exact: true }).fill(value);
    await page.getByRole('button', { name: 'Save CSS', exact: true }).click();
    await page.waitForFunction(() => saveCalls.at(-1)?.raw === document.getElementById('customCSSBox')?.value);
  }

  await beginSave('.reply { background-color: #334455; }');
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  assert.equal(await page.evaluate(() => saveCalls.at(-1).aborted), true);
  await page.evaluate(() => releaseCustomSave?.());
  assert.equal(await page.evaluate(() => cssValue), '.reply { background-color: #112233; padding-left: 8px; }');

  await beginSave('.reply { background-color: #445566; }');
  await page.evaluate(() => {
    settingsState = { customCSS: false };
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.equal(await page.evaluate(() => saveCalls.at(-1).aborted), true);
  await page.evaluate(() => releaseCustomSave?.());
  assert.equal(await page.evaluate(() => cssValue), '.reply { background-color: #112233; padding-left: 8px; }');

  await page.evaluate(() => { settingsState = { customCSS: false, disableAll: false }; customCSS.refresh(); });
  await beginSave('.reply { background-color: #4a5b6c; }');
  await page.evaluate(() => {
    settingsState = { customCSS: false, disableAll: true };
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  assert.equal(await page.evaluate(() => saveCalls.at(-1).aborted), true);
  await page.evaluate(() => releaseCustomSave?.());
  const callsBeforeDisabledSubmit = await page.evaluate(() => saveCalls.length);
  await page.getByRole('button', { name: 'Save CSS', exact: true }).click();
  assert.equal(await page.evaluate(() => saveCalls.length), callsBeforeDisabledSubmit);
  assert.match(await page.getByRole('status').textContent(), /cannot be saved while native features are disabled/i);
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();

  await page.evaluate(() => { settingsState = { customCSS: true }; customCSS.refresh(); });
  await beginSave('.reply { background-color: #556677; }');
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  assert.equal(await page.evaluate(() => saveCalls.at(-1).aborted), true);
  assert.equal(await page.locator('#customCSSMenu').count(), 0);
  await page.evaluate(() => releaseCustomSave?.());
  assert.equal(await page.evaluate(() => cssValue), '.reply { background-color: #112233; padding-left: 8px; }');
});

test('pending editor save uses the submitted snapshot and empty saves compare as missing storage', async t => {
  const { page } = await fixture(t);
  await page.evaluate(() => { saveMode = 'delayed'; customCSS.open(document.getElementById('opener')); });
  const input = page.getByLabel('Post CSS', { exact: true });
  const first = '.reply { color: #334455; }';
  const editedWhilePending = '.reply { color: #445566; }';
  await input.fill(first);
  await page.getByRole('button', { name: 'Save CSS', exact: true }).click();
  await page.waitForFunction(() => saveCalls.length === 1);
  await input.fill(editedWhilePending);
  await page.evaluate(() => releaseCustomSave());
  await page.waitForFunction(() => document.querySelector('#customCSSMenu button[type=submit]')?.disabled === false);
  assert.equal(await page.evaluate(() => cssValue), first);

  await page.evaluate(() => { saveMode = 'immediate'; });
  await page.getByRole('button', { name: 'Save CSS', exact: true }).click();
  await page.waitForFunction(() => saveCalls.length === 2);
  assert.deepEqual(await page.evaluate(() => ({ expected: saveCalls[1].expected, stored: cssValue })), {
    expected: first, stored: editedWhilePending,
  });

  await input.fill('');
  await page.getByRole('button', { name: 'Save CSS', exact: true }).click();
  await page.waitForFunction(() => saveCalls.length === 3);
  assert.deepEqual(await page.evaluate(() => ({ expected: saveCalls[2].expected, raw: saveCalls[2].raw, stored: cssValue })), {
    expected: editedWhilePending, raw: '', stored: null,
  });

  const afterClear = '.postMessage { color: #556677; }';
  await input.fill(afterClear);
  await page.getByRole('button', { name: 'Save CSS', exact: true }).click();
  await page.waitForFunction(() => saveCalls.length === 4);
  assert.deepEqual(await page.evaluate(() => ({ expected: saveCalls[3].expected, stored: cssValue })), {
    expected: null, stored: afterClear,
  });
});

test('detaching the owned board aborts the editor and removes the document stylesheet', async t => {
  const { page } = await fixture(t);
  await page.evaluate(() => { saveMode = 'delayed'; customCSS.open(document.getElementById('opener')); });
  await page.getByLabel('Post CSS', { exact: true }).fill('.reply { background-color: #667788; }');
  await page.getByRole('button', { name: 'Save CSS', exact: true }).click();
  await page.waitForFunction(() => saveCalls.length === 1);
  await page.evaluate(() => document.querySelector('.board').remove());
  await page.waitForFunction(() => saveCalls[0].aborted === true);
  assert.equal(await page.locator('#customCSSMenu').count(), 0);
  assert.equal(await page.evaluate(() => document.adoptedStyleSheets.filter(sheet => [...sheet.cssRules]
    .some(rule => rule.cssText.includes('.board .post.reply'))).length), 0);
  await page.evaluate(() => {
    const replacement = document.createElement('main'); replacement.className = 'board';
    const post = document.createElement('div'); post.id = 'replacement'; post.className = 'post reply';
    replacement.append(post); document.body.append(replacement); releaseCustomSave?.();
  });
  assert.equal(await page.locator('#replacement').evaluate(element => getComputedStyle(element).backgroundColor), 'rgba(0, 0, 0, 0)');
  assert.equal(await page.evaluate(() => cssValue), '.reply { background-color: #112233; padding-left: 8px; }');
});

test('editor rejects invalid drafts and refuses stale cross-tab overwrite', async t => {
  const { page, requests } = await fixture(t);
  await page.evaluate(() => customCSS.open(document.getElementById('opener')));
  assert.match(await page.locator('.customCSSHelp').textContent(), /\.postMessage \{ color: #336699; font-size: 14px; \}/);
  await page.getByLabel('Post CSS', { exact: true }).fill('.reply { display: none; background-image: url(https://attacker.invalid/a.png); }');
  await page.getByRole('button', { name: 'Save CSS', exact: true }).click();
  assert.match(await page.getByRole('status').textContent(), /not allowed|functions are not allowed/i);
  assert.equal(await page.evaluate(() => saveCalls.length), 0);
  assert.deepEqual(requests, []);

  await page.getByLabel('Post CSS', { exact: true }).fill('.reply { color: #334455; }');
  await page.evaluate(() => {
    cssValue = '.reply { color: #abcdef; }';
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan-css' }));
  });
  await page.getByRole('button', { name: 'Save CSS', exact: true }).click();
  assert.match(await page.getByRole('status').textContent(), /changed in another tab/i);
  assert.equal(await page.evaluate(() => cssValue), '.reply { color: #abcdef; }');
  assert.equal(await page.evaluate(() => saveCalls.at(-1).expected), '.reply { background-color: #112233; padding-left: 8px; }');
  assert.equal(await page.locator('#customCSSMenu').count(), 1);
});
