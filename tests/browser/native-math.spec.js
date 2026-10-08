import { test, expect } from '@playwright/test';

// Run with the dedicated math-browser-fixture server. These tests use the real
// released worker/renderer, production templates, and production CSP headers.
const origin = 'http://127.0.0.1:3000';
const pagePath = '/sci/thread/1000001';
const modulePath = '/static/native-math.v1.js';
const workerPath = '/static/native-math-worker.v1.js';
const source = 'Before [math]x^2+\\frac{1}{2}[/math] after\n[eqn]\\sum_{i=1}^{n} i=\\frac{n(n+1)}{2}[/eqn]';
const unexpected = new WeakMap();

test.beforeEach(async ({ context, page }) => {
  const outside = []; unexpected.set(page, outside);
  context.on('request', request => { if (new URL(request.url()).origin !== origin) outside.push(request.url()); });
  await context.route('**/*', route => new URL(route.request().url()).origin === origin ? route.continue() : route.abort());
  await page.addInitScript(() => {
    // A test-specific init script may run before or after this default.
    if (localStorage.getItem('4chan-settings') === null) {
      localStorage.setItem('4chan-settings', JSON.stringify({ threadStats: false, autoUpdate: false }));
    }
    window.mathCspViolations = [];
    document.addEventListener('securitypolicyviolation', event => {
      window.mathCspViolations.push({ directive: event.effectiveDirective, blocked: event.blockedURI });
    });
  });
});
test.afterEach(async ({ page }) => { expect(unexpected.get(page)).toEqual([]); });

async function state(page) {
  return page.evaluate(async path => {
    const { pageNativeMath } = await import(path);
    const instance = pageNativeMath();
    const message = document.getElementById('m1000001');
    return instance && { text: instance.projection.text(message, '\n'), html: instance.projection.html(message),
      clone: instance.projection.clone(message).textContent };
  }, modulePath);
}
async function addMessage(page, text, id = 'dynamic-math') {
  await page.evaluate(({ text, id }) => {
    const message = document.createElement('blockquote'); message.className = 'postMessage'; message.id = id;
    message.textContent = text; document.querySelector('.board').append(message);
    message.scrollIntoView({ block: 'center' });
  }, { text, id });
  return page.locator(`#${id}`);
}

test('real inline and block geometry renders under the actual response CSP and retains literal source', async ({ page }) => {
  const response = await page.goto(pagePath);
  const csp = response.headers()['content-security-policy'];
  expect(csp).toBeTruthy();
  expect(csp).not.toMatch(/unsafe-eval|unsafe-inline/);
  expect(csp).toContain(`${origin}${workerPath}`);
  await expect(page.locator('#m1000001 .nativeMath svg')).toHaveCount(2);
  await expect(page.locator('#m1000001 .nativeMath:not(.displayMath) svg')).toHaveCount(1);
  await expect(page.locator('#m1000001 .displayMath svg')).toHaveCount(1);
  expect(await page.locator('#m1000001 svg path').count()).toBeGreaterThan(2);
  const display = await page.locator('#m1000001 .displayMath').evaluate(node => ({
    display: getComputedStyle(node).display, textAlign: getComputedStyle(node).textAlign,
  }));
  expect(display.display).toBe('block');
  expect(display.textAlign).not.toBe('center');
  expect((await state(page)).text).toBe(source);
  await expect(page.locator('#m1000001').getByRole('math').first()).toHaveAttribute('aria-label', 'x^2+\\frac{1}{2}');
  expect((await state(page)).html).not.toMatch(/svg|nativeMath|data-mjx/);
  expect((await state(page)).clone).toContain('[math]x^2+\\frac{1}{2}[/math]');
  expect(await page.evaluate(() => window.mathCspViolations)).toEqual([]);
});

test('core math remains enabled independently of disableAll, but disabled board and catalog never start workers', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await page.goto(pagePath);
  await expect(page.locator('#m1000001 svg')).toHaveCount(2);
  for (const path of ['/test/thread/1000001', '/sci/catalog']) {
    const workers = [];
    const listener = request => { if (request.url().endsWith(workerPath)) workers.push(request.url()); };
    page.on('request', listener);
    const response = await page.goto(path);
    await expect(page.locator('.nativeMath')).toHaveCount(0);
    expect(await page.locator('body').getAttribute('data-math-tags')).not.toBe('1');
    expect(response.headers()['content-security-policy']).not.toContain(workerPath);
    expect(workers).toEqual([]);
    page.off('request', listener);
  }
});

test('the real QR TeX control owns a separate debounced input and discards closed preview work', async ({ page }) => {
  await page.clock.install({ time: new Date('2026-09-08T12:00:00Z') });
  await page.goto(pagePath);
  await expect(page.locator('#m1000001 .nativeMath > svg')).toHaveCount(2);
  // install() alone keeps time flowing. Pause before asserting exact 50 ms
  // boundaries so slower host interactions cannot advance the debounce.
  await page.clock.pauseAt(new Date('2026-09-08T12:05:00Z'));
  await page.locator('#pi1000001 a[title="Reply to this post"]').click();
  await page.locator('#qrCom').fill('Untouched posting draft [math]z[/math]');
  await page.getByRole('button', { name: 'Preview TeX equations' }).click();
  await expect(page.locator('#input-tex-preview')).toHaveValue('');
  await page.locator('#input-tex-preview').fill('[math]a^2[/math]');
  await page.clock.runFor(30);
  await expect(page.locator('#output-tex-preview svg')).toHaveCount(0);
  await page.locator('#input-tex-preview').fill('[eqn]\\frac{a}{b}[/eqn]');
  await page.clock.runFor(49);
  await expect(page.locator('#output-tex-preview svg')).toHaveCount(0);
  await page.clock.runFor(2);
  await expect(page.locator('#output-tex-preview svg')).toHaveCount(1);
  await expect(page.locator('#qrCom')).toHaveValue('Untouched posting draft [math]z[/math]');
  await page.locator('#input-tex-preview').fill('[math]stale[/math]');
  await page.getByRole('button', { name: 'Close TeX preview', exact: true }).click();
  await page.clock.runFor(100);
  await expect(page.locator('#tex-preview-cnt')).toHaveCount(0);
  await page.getByRole('button', { name: 'Preview TeX equations' }).click();
  await expect(page.locator('#input-tex-preview')).toHaveValue('');
  await expect(page.locator('#output-tex-preview svg')).toHaveCount(0);
});

test('plain pages stay lazy and newly inserted or replaced comments get real typesetting', async ({ page }) => {
  const workers = [];
  page.on('request', request => { if (request.url().endsWith(workerPath)) workers.push(request.url()); });
  await page.goto('/sci/thread/1000002');
  expect(workers).toEqual([]);
  const message = await addMessage(page, '[math]\\sqrt{x}[/math]');
  await expect(message.locator('svg')).toHaveCount(1);
  expect(workers.length).toBeGreaterThan(0);
  await message.evaluate(node => { node.textContent = '[eqn]\\frac{a}{b}[/eqn]'; });
  await expect(message.locator('.displayMath svg')).toHaveCount(1);
  await message.evaluate(node => node.remove());
  await expect(page.locator('#dynamic-math')).toHaveCount(0);
  await addMessage(page, '[math]q^2[/math]');
  await expect(page.locator('#dynamic-math svg')).toHaveCount(1);
});

test('typesetting preserves visible prose line breaks around an equation', async ({ page }) => {
  await page.goto('/sci/thread/1000002');
  const message = await addMessage(page, '');
  await message.evaluate(node => {
    node.append('first line', document.createElement('br'), '[math]x[/math]', document.createElement('br'), 'last line');
  });
  await expect(message.locator('.nativeMath > svg')).toHaveCount(1);
  await expect(message.locator('.nativeMathRun > br')).toHaveCount(2);
  const positions = await message.locator('.nativeMathRun').evaluate(node => {
    const text = [...node.childNodes].filter(child => child.nodeType === 3);
    const top = child => { const range = document.createRange(); range.selectNodeContents(child); return range.getBoundingClientRect().top; };
    return [top(text[0]), node.querySelector('svg').getBoundingClientRect().top, top(text.at(-1))];
  });
  expect(positions[1]).toBeGreaterThan(positions[0]);
  expect(positions[2]).toBeGreaterThan(positions[1]);
});

test('late linkification and mobile defaults replan surrounding source without losing math', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: true, linkify: false })));
  await page.goto('/sci/thread/1000002');
  const message = await addMessage(page, '[math]x[/math] https://example.com');
  await expect(message.locator('.nativeMath > svg')).toHaveCount(1);
  await expect(message.locator('a.linkified')).toHaveCount(0);
  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: true, linkify: true }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  await expect(message.locator('a.linkified')).toHaveCount(1);
  await expect(message.locator('.nativeMath > svg')).toHaveCount(1);
  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: true, linkify: false }));
    document.dispatchEvent(new CustomEvent('4chanSettingsSaved'));
  });
  await expect(message.locator('a.linkified')).toHaveCount(0);
  await expect(message.locator('.nativeMath > svg')).toHaveCount(1);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(message.locator('a.linkified')).toHaveCount(1);
  await expect(message.locator('.nativeMath > svg')).toHaveCount(1);
});

test('delayed real worker startup cannot commit to a removed or replaced comment', async ({ page }) => {
  let release;
  const held = new Promise(resolve => { release = resolve; });
  await page.route(`**${workerPath}`, async route => { await held; await route.continue(); });
  const request = page.waitForRequest(`**${workerPath}`);
  await page.goto(pagePath);
  await request;
  await page.locator('#m1000001').evaluate(node => {
    window.removedMathMessage = node;
    const replacement = document.createElement('blockquote'); replacement.id = node.id;
    replacement.className = 'postMessage'; replacement.textContent = 'Replacement remains plain';
    node.replaceWith(replacement);
  });
  release();
  await expect(page.locator('#m1000001')).toHaveText('Replacement remains plain');
  await page.waitForTimeout(1700); // Beyond the documented per-job deadline.
  expect(await page.evaluate(() => window.removedMathMessage.querySelectorAll('svg').length)).toBe(0);
  await expect(page.locator('#m1000001 svg')).toHaveCount(0);
});

test('pagehide restores literal comments and pageshow retypesets without retaining stale geometry', async ({ page }) => {
  await page.goto(pagePath);
  await expect(page.locator('#m1000001 svg')).toHaveCount(2);
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  await expect(page.locator('#m1000001 svg')).toHaveCount(0);
  await expect(page.locator('#m1000001')).toContainText('[math]x^2+\\frac{1}{2}[/math]');
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
  await expect(page.locator('#m1000001 svg')).toHaveCount(2);
  expect((await state(page)).text).toBe(source);
});

test('hostile resource macros never introduce active DOM or resource requests; bounded failure preserves text', async ({ page }) => {
  await page.goto('/sci/thread/1000002');
  const requests = [];
  page.on('request', request => requests.push(new URL(request.url()).pathname));
  const hostile = String.raw`[math]\href{https://evil.invalid/x}{x}\includegraphics{https://evil.invalid/y}\require{https://evil.invalid/z}\class{owned}{x}\cssId{owned}{x}\style{background:url(https://evil.invalid/a)}{x}\def\x{\x}\x[/math]`;
  await addMessage(page, hostile);
  await page.waitForTimeout(1700);
  expect(requests.filter(path => !path.endsWith(workerPath))).toEqual([]);
  await expect(page.locator('#dynamic-math a, #dynamic-math img, #dynamic-math iframe, #dynamic-math script, #dynamic-math style, #dynamic-math foreignObject')).toHaveCount(0);
  const snapshot = await page.evaluate(async path => {
    const instance = (await import(path)).pageNativeMath();
    return instance.projection.text(document.getElementById('dynamic-math'));
  }, modulePath);
  expect(snapshot).toBe(hostile);
  const oversized = `[math]${'x+'.repeat(40000)}[/math]`;
  await page.locator('#dynamic-math').evaluate((node, text) => { node.textContent = text; }, oversized);
  await expect(page.locator('#dynamic-math svg')).toHaveCount(0);
  await expect(page.locator('#dynamic-math')).toHaveText(oversized);
});

test('unavailable worker leaves literal source and a responsive page without endless retries', async ({ page }) => {
  const requests = [];
  await page.route(`**${workerPath}`, route => { requests.push(route.request().url()); return route.abort(); });
  await page.goto(pagePath);
  await page.waitForTimeout(1800);
  await expect(page.locator('#m1000001')).toContainText('[math]x^2+\\frac{1}{2}[/math]');
  await expect(page.locator('#m1000001 svg')).toHaveCount(0);
  expect(requests.length).toBeGreaterThan(0);
  expect(requests.length).toBeLessThanOrEqual(4);
  await page.locator('#togglePostFormLink a').click();
  await page.locator('#com').fill('Page still responds');
  await expect(page.locator('#com')).toHaveValue('Page still responds');
});

test('typeset source still drives real quote previews, inline copies, backlinks and comment filters', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ inlineQuotes: true, threadStats: false, autoUpdate: false })));
  await page.goto(pagePath);
  await expect(page.locator('#m1000001 svg')).toHaveCount(2);
  await expect(page.locator('#bl_1000001 a.quotelink[href="/sci/thread/1000001#p1000003"]')).toHaveCount(1);
  const link = page.locator('#m1000003 a.quotelink').first();
  // A fully visible local target is highlighted instead of copied by design.
  // Separate the reply spatially so this case exercises the actual popup path.
  await page.locator('#pc1000003').evaluate(node => { node.style.marginTop = '1200px'; });
  await link.hover();
  expect(await page.locator('#p1000001').evaluate(node => node.getBoundingClientRect().top)).toBeLessThan(0);
  await expect(page.locator('#quote-preview .postMessage svg')).toHaveCount(2);
  await page.mouse.move(0, 0);
  await expect(page.locator('#quote-preview')).toHaveCount(0);
  await link.click();
  await expect(page.locator('.inlined .postMessage svg')).toHaveCount(2);
  await expect(page.locator('#bl_1000001 a.quotelink[href="/sci/thread/1000001#p1000003"]')).toHaveCount(1);
  // Apply the real filter after rendering, when ordinary DOM text no longer
  // contains the equation. A literal TeX match requires the shared projection.
  await page.evaluate(() => {
    localStorage.setItem('4chan-filters', JSON.stringify([{
      type: 2, pattern: '"[math]y^2[/math]"', boards: 'sci', active: true,
      auto: false, hide: false, color: '#ff0000',
    }]));
    localStorage.setItem('4chan-settings', JSON.stringify({ filter: true, threadStats: false, autoUpdate: false }));
    document.dispatchEvent(new Event('4chanSettingsSaved'));
  });
  await expect(page.locator('#p1000003')).toHaveClass(/filter-hl/);
  expect((await state(page)).text).toBe(source);
});

async function snapshotFromProduction(page, request, insertedId) {
  const html = await (await request.get(pagePath)).text();
  return page.evaluate(({ html, insertedId }) => {
    const document = new DOMParser().parseFromString(html, 'text/html');
    const posts = [...document.querySelectorAll('.board .postContainer')].map(node => ({
      no: node.id.slice(2), file_deleted: false, html: node.outerHTML,
    }));
    const reply = document.getElementById('pc1000003').cloneNode(true);
    for (const node of [reply, ...reply.querySelectorAll('*')]) {
      for (const attr of [...node.attributes]) node.setAttribute(attr.name, attr.value.replaceAll('1000003', insertedId));
    }
    for (const number of reply.querySelectorAll('.postNum a[title="Reply to this post"]')) number.textContent = insertedId;
    reply.querySelector('.postMessage').textContent = '[eqn]\\frac{u}{v}[/eqn]';
    posts.push({ no: insertedId, file_deleted: false, html: reply.outerHTML });
    posts.sort((a, b) => Number(a.no) - Number(b.no));
    return { version: 2, board: 'sci', thread: '1000001', closed: false, archived: false, sticky: false,
      replies: 2, images: 0, tail_size: 0, tail_id: null, posts };
  }, { html, insertedId });
}

test('real updater and expansion pipelines typeset literal math received in bounded snapshots', async ({ page, request }) => {
  await page.goto(pagePath);
  await expect(page.locator('#m1000001 svg')).toHaveCount(2);
  let snapshot = await snapshotFromProduction(page, request, '1000004');
  await page.route('**/_watch/sci/thread/1000001/posts', route => route.fulfill({
    status: 200, contentType: 'application/json', body: JSON.stringify(snapshot),
  }));
  await page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
  await expect(page.locator('#m1000004')).toBeVisible();
  await expect(page.locator('#m1000004 .displayMath svg')).toHaveCount(1);
  expect((await state(page)).text).toBe(source);
  await page.goto('/sci/');
  snapshot = await snapshotFromProduction(page, request, '1000002');
  await page.getByRole('button', { name: 'Expand thread 1000001', exact: true }).click();
  await expect(page.locator('#m1000002 .displayMath svg')).toHaveCount(1);
  await page.getByRole('button', { name: 'Collapse thread 1000001', exact: true }).click();
  await expect(page.locator('#m1000002')).toBeHidden();
  await page.getByRole('button', { name: 'Expand thread 1000001', exact: true }).click();
  await expect(page.locator('#m1000002 .displayMath svg')).toHaveCount(1);
});
