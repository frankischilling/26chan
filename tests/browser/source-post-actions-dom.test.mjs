import test, { before, after } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

const source = await readFile(new URL('../../apps/public/static/source-post-actions.v1.js', import.meta.url), 'utf8');
const large = '9223372036854775807';
let browser;
before(async () => { browser = await chromium.launch({ headless: true }); });
after(async () => { await browser?.close(); });

async function fixture(t, ids = [large, '2']) {
  const context = await browser.newContext();
  t.after(() => context.close());
  const page = await context.newPage();
  await page.route('**/*', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><html><body></body></html>' }));
  await page.goto('https://boards.test/demo/thread/1');
  await page.evaluate(async ({ ids, source }) => {
    document.body.innerHTML = `<main class="board"><section class="thread" id="t1" data-archived="false">${ids.map(id =>
      `<article class="postContainer replyContainer" id="pc${id}"><div class="post reply" id="p${id}"><div class="postInfo" id="pi${id}"><input type="checkbox" id="select${id}" name="${id}" value="delete" form="delform" aria-label="Select post No.${id}"></div><blockquote>Owned</blockquote></div></article>`).join('')}</section></main>
      <form name="delform" id="delform" action="/demo/imgboard.php" method="post" data-board="demo">
      <input type="hidden" name="mode" value="usrdel"><input type="checkbox" name="onlyimgdel" value="on">
      <input type="hidden" id="delPassword" name="pwd"><input type="submit" value="Delete">
      <input id="bottomReportBtn" type="button" value="Report"><span id="deleteSelectionStatus" role="status"></span></form>`;
    const url = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
    try { window.actions = await import(url); } finally { URL.revokeObjectURL(url); }
    window.form = document.getElementById('delform');
    window.submitSelection = (submitter = form.querySelector('input[type="submit"]')) => {
      const event = new SubmitEvent('submit', { bubbles: true, cancelable: true, submitter });
      form.dispatchEvent(event);
      return { refused: event.defaultPrevented, fields: [...new FormData(form)], message: document.getElementById('deleteSelectionStatus').textContent };
    };
    window.opened = [];
    window.open = (...args) => { opened.push(args); return {}; };
  }, { ids, source });
  return page;
}

test('native form owns exact selected IDs in document order, preserves controls and refuses a duplicate submission', async t => {
  const page = await fixture(t);
  for (const input of await page.locator('input[form="delform"]').all()) await input.check();
  const first = await page.evaluate(() => submitSelection());
  assert.equal(first.refused, false);
  assert.deepEqual(first.fields, [[large, 'delete'], ['2', 'delete'], ['mode', 'usrdel'], ['pwd', '']]);
  assert.match(first.message, /Earlier items may succeed/);
  const second = await page.evaluate(() => submitSelection());
  assert.equal(second.refused, true);
  assert.match(second.message, /Reload before trying again/);
  assert.equal(await page.locator('.postContainer.deleted').count(), 0);
  assert.equal(await page.locator('input:disabled').count(), 0);
});

test('empty and over-budget selections never start native submission', async t => {
  const ids = Array.from({ length: 19 }, (_, index) => String(index + 2));
  const page = await fixture(t, ids);
  assert.equal((await page.evaluate(() => submitSelection())).refused, true);
  await page.locator('input[form="delform"]').evaluateAll(nodes => nodes.forEach(node => { node.checked = true; }));
  assert.match((await page.evaluate(() => submitSelection())).message, /at most 18/);
  await page.locator('#select20').uncheck();
  await page.locator('[name="onlyimgdel"]').check();
  assert.match((await page.evaluate(() => submitSelection())).message, /at most 17/);
  await page.locator('#select19').uncheck();
  const accepted = await page.evaluate(() => submitSelection());
  assert.equal(accepted.refused, false);
  assert.equal(accepted.fields.length, 20);
  assert.ok(accepted.fields.some(([key, value]) => key === 'onlyimgdel' && value === 'on'));
});

test('report uses only the first checked original post and never submits deletion', async t => {
  const page = await fixture(t);
  await page.locator('#bottomReportBtn').click();
  assert.equal(await page.evaluate(() => opened.length), 0);
  await page.locator('input[form="delform"]').evaluateAll(nodes => nodes.forEach(node => { node.checked = true; }));
  await page.locator('#bottomReportBtn').click();
  const opened = await page.evaluate(() => window.opened);
  assert.equal(opened.length, 1);
  assert.equal(opened[0][0], `/demo/imgboard.php?mode=report&no=${large}`);
  assert.ok(opened[0][1].startsWith(`report-popup-demo-${large}-`));
  assert.equal(await page.locator('#delform').getAttribute('aria-busy'), null);
});

test('form overrides, foreign controls, cloned selectors and moved targets fail closed', async t => {
  const mutations = [
    () => form.setAttribute('action', '/other/imgboard.php'),
    () => form.setAttribute('target', '_blank'),
    () => form.querySelector('[type="submit"]').setAttribute('formaction', '/demo/delete'),
    () => form.querySelector('[name="mode"]').value = 'regist',
    () => form.querySelector('[name="pwd"]').name = 'password',
    () => { const input = document.createElement('input'); input.name = 'resto'; form.append(input); },
    () => document.body.append(document.querySelector('[form="delform"]').cloneNode(true)),
    () => document.body.append(document.querySelector('.postContainer')),
    () => document.querySelector('.thread').dataset.archived = 'true',
    () => document.querySelector('.postContainer').classList.add('deleted'),
  ];
  for (const mutate of mutations) {
    const page = await fixture(t);
    await page.locator('input[form="delform"]').first().check();
    await page.evaluate(mutate);
    assert.equal((await page.evaluate(() => submitSelection())).refused, true, String(mutate));
    await page.context().close();
  }
});

test('unselected retired posts do not block a healthy selection and bfcache never retries a submitted batch', async t => {
  const page = await fixture(t);
  await page.evaluate(() => document.querySelector('.postContainer').classList.add('deleted'));
  await page.locator('#select2').check();
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  assert.equal((await page.evaluate(() => submitSelection())).refused, true);
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  const accepted = await page.evaluate(() => submitSelection());
  assert.equal(accepted.refused, false);
  assert.deepEqual(accepted.fields, [['2', 'delete'], ['mode', 'usrdel'], ['pwd', '']]);
  await page.evaluate(() => {
    window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
    window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
  });
  assert.equal((await page.evaluate(() => submitSelection())).refused, true);
});
