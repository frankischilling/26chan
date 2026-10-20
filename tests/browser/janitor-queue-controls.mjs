import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium, expect } from '@playwright/test';

const script = await readFile('apps/staff/static/request-queue.js', 'utf8');
const css = await readFile('apps/staff/static/reportqueue.v1.css', 'utf8');
const reference = JSON.parse(await readFile('fixtures/janitor-queue-filter.json', 'utf8'));
const origin = 'http://localhost:3001';
const row = (id, name, comment, template) => `<tr class="ban-request" id="ban-request-${id}" tabindex="-1"><td class="post-name">${name}</td><td class="post-content">${comment}</td><td class="ban-request-template">${template}</td><td><form action="/ban-requests/review" method="post"><input name="id" value="${id}" type="hidden"><input name="csrf" value="owned-fixture" type="hidden"><button name="decision" value="accept">Accept</button><button name="decision" value="deny">Deny</button></form><details class="request-amend"><summary>Amend</summary><form action="/ban-requests/review" method="post"><input name="id" value="${id}" type="hidden"><input name="decision" value="accept" type="hidden"><select name="amend_tpl"><option value="1">Owned</option></select><textarea name="public_reason"></textarea><button>Accept amendment</button></form></details></td></tr>`;
const content = () => `<div id="content" data-board="g"><table id="ban-requests"><tbody id="br-list">${row(1, 'Alice', 'Alpha &lt;script&gt;owned&lt;/script&gt;', 'Spam')}${row(2, 'Bob', 'Beta', 'Off topic')}</tbody></table></div>`;
const html = () => `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="referrer" content="same-origin"><link rel="stylesheet" href="/reportqueue.css"><script defer src="/request-queue.js"></script></head><body><header><form id="filter-form" hidden><input id="search-box" aria-label="Filter loaded requests"><button>Filter</button><button id="reset-btn" type="reset" hidden>Reset</button></form><h1 id="title">Ban Requests</h1><div id="queue-preferences" hidden><input id="cfg-cb-dt" type="checkbox"><input id="queue-keybinds" type="checkbox" checked></div></header><a id="refresh-btn" href="/ban-requests?board=g">Refresh</a><ul id="board-menu"><li>g</li></ul><p id="queue-status" role="status"></p><button id="queue-undo" hidden>Undo pending review</button>${content()}</body></html>`;
const browser = await chromium.launch(process.env.CHROMIUM_EXECUTABLE_PATH ? { executablePath: process.env.CHROMIUM_EXECUTABLE_PATH } : {});
try {
  for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
    const context = await browser.newContext({ viewport });
    const page = await context.newPage();
    let writes = [];
    let fail = false;
    let failRefresh = false;
    let delayedRead = null;
    let delayNext = false;
    await context.route('**/*', async route => {
      const url = new URL(route.request().url());
      assert.equal(url.origin, origin);
      if (url.pathname === '/request-queue.js') return route.fulfill({ contentType: 'text/javascript', body: script });
      if (url.pathname === '/reportqueue.css') return route.fulfill({ contentType: 'text/css', body: css });
      if (url.pathname === '/ban-requests/review') {
        writes.push(route.request().postData());
        if (fail) return route.abort();
        return route.fulfill({ contentType: 'application/json', body: JSON.stringify({ changed: true, cleared: 1, ban_id: null }) });
      }
      if (delayNext) { delayNext = false; delayedRead = route; return; }
      return route.fulfill({ status: failRefresh ? 403 : 200, contentType: 'text/html', body: html() });
    });
    await page.goto(`${origin}/ban-requests?board=g`);
    await expect(page.locator('#filter-form')).toBeVisible();
    // Execute only the isolated source filter against the same harmless fixture.
    const expected = await page.evaluate(code => {
      const BR = {};
      const root = document.createElement('div');
      root.append(document.getElementById('content').cloneNode(true));
      root.append(document.getElementById('reset-btn').cloneNode(true));
      const $ = { id: id => root.querySelector(`#${id}`), cls: (name, scope = root) => scope.getElementsByClassName(name) };
      new Function('BR', '$', code)(BR, $);
      return ['', 'ALICE', 'beta', 'off topic', '<script>', 'g', 'nothing', ' '].map(query => {
        BR.applySearch(query);
        const ids = [...root.querySelectorAll('.ban-request')].filter(row => !row.classList.contains('hidden')).map(row => row.id);
        BR.clearSearch();
        return { query, ids };
      });
    }, reference.functions);
    for (const { query, ids } of expected) {
      await page.locator('#search-box').fill(query);
      await page.locator('#search-box').press('Enter');
      assert.deepEqual(await page.locator('.ban-request:visible').evaluateAll(rows => rows.map(row => row.id)), ids);
    }
    await page.locator('#reset-btn').click();
    await expect(page.locator('.ban-request:visible')).toHaveCount(2);
    await page.locator('#cfg-cb-dt').check();
    await expect(page.locator('html')).toHaveClass('dark-theme');
    await page.reload();
    await expect(page.locator('#cfg-cb-dt')).toBeChecked();
    await page.locator('#ban-request-1').focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('#ban-request-1')).toBeFocused();
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('#ban-request-2')).toBeFocused();
    await page.keyboard.press('ArrowLeft');
    await expect(page.locator('#ban-request-1')).toBeFocused();
    await page.keyboard.press('c');
    await expect(page.locator('#ban-request-1 select')).toBeFocused();
    await page.keyboard.press('d');
    assert.equal(writes.length, 0, 'select interaction must not trigger a review');
    await page.keyboard.press('Escape');
    await expect(page.locator('#ban-request-1 details')).not.toHaveAttribute('open');
    // Keep host scheduling between 2999 ms and Undo from crossing the 3 s
    // deadline. runFor advances this paused clock explicitly.
    await page.clock.pauseAt(new Date('2026-09-13T19:00:00Z'));
    await page.keyboard.press('d');
    await expect(page.locator('#queue-undo')).toBeVisible();
    await page.clock.runFor(2999);
    assert.equal(writes.length, 0);
    await page.keyboard.press('u');
    await page.clock.runFor(4000);
    assert.equal(writes.length, 0, 'Undo must cancel the actual request');
    await page.locator('#ban-request-1 button[value="deny"]').dblclick();
    await page.clock.runFor(3001);
    await expect.poll(() => writes.length).toBe(1);
    await expect(page.locator('#queue-status')).toContainText('Request denied.');
    assert.equal(new URLSearchParams(writes[0]).get('decision'), 'deny');
    // A lost response never causes an automatic retry, including repeated clicks.
    fail = true;
    await page.locator('#ban-request-1 button[value="accept"]').click();
    await page.clock.runFor(3001);
    await expect(page.locator('#queue-status')).toContainText('could not be confirmed');
    const failedCount = writes.length;
    await page.locator('#ban-request-1 button[value="accept"]').click();
    await page.clock.runFor(5000);
    assert.equal(writes.length, failedCount);
    fail = false;
    await page.locator('#refresh-btn').click();
    await expect(page.locator('#queue-status')).toHaveText('Requests refreshed.');
    // A superseded refresh cannot replace a newer response.
    delayNext = true;
    await page.locator('#refresh-btn').click();
    await expect.poll(() => delayedRead !== null).toBe(true);
    await page.locator('#refresh-btn').click();
    await expect(page.locator('#queue-status')).toHaveText('Requests refreshed.');
    await delayedRead.fulfill({ contentType: 'text/html', body: html().replace('data-board="g"', 'data-board="stale"') }).catch(() => {});
    assert.equal(await page.locator('#content').getAttribute('data-board'), 'g');
    // A confirmed mutation followed by an expired read still blocks stale controls.
    failRefresh = true;
    await page.locator('#ban-request-1 button[value="deny"]').click();
    await page.clock.runFor(3001);
    await expect(page.locator('#queue-status')).toContainText('Reload before another review');
    const confirmedCount = writes.length;
    await page.locator('#ban-request-1 button[value="deny"]').click();
    await page.clock.runFor(4000);
    assert.equal(writes.length, confirmedCount);
    failRefresh = false;
    await page.reload();
    // Navigating away cancels an unsent timer.
    await page.locator('#ban-request-1 button[value="deny"]').click();
    await page.goto(`${origin}/ban-requests?board=g`);
    await page.clock.runFor(4000);
    assert.equal(writes.length, confirmedCount);
    await context.close();
    console.log(`PASS ${viewport.width}: source filter, theme, focus, amendment, delayed Undo, double submit, lost response, stale refresh and navigation`);
  }
} finally { await browser.close(); }
