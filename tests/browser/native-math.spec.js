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
  await page.addInitScript(() => {
    window.initialMathPageParsed = false;
    document.addEventListener('4chanParsingDone', event => {
      if (event.detail?.offset === 0 &&
          event.detail.threadId === document.getElementById('watcher-context')?.dataset.thread) {
        window.initialMathPageParsed = true;
      }
    });
  });
  await page.goto('/sci/thread/1000002');
  await expect.poll(() => page.evaluate(() => window.initialMathPageParsed)).toBe(true);
  // Initial watcher decoration is unrelated to the hostile expression below.
  // Finish its actual icon loads before measuring expression-triggered requests.
  const icons = page.locator('#twPrune img, #twClose img');
  await expect(icons).toHaveCount(2);
  await icons.evaluateAll(images => Promise.all(images.map(image => image.decode())));
  expect(await icons.evaluateAll(images => images.every(image => image.complete && image.naturalWidth > 0))).toBe(true);
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

async function listenForMainInit(page, hideOnMain = false) {
  await page.addInitScript(hide => {
    window.mainInitTrace = [];
    const Worker = window.Worker;
    window.Worker = class extends Worker {
      constructor(url, options) {
        if (String(url).includes('native-math-worker')) mainInitTrace.push({ mathWorker: true });
        super(url, options);
      }
    };
    document.addEventListener('4chanMainInit', event => {
      mainInitTrace.push({ main: true, constructor: event.constructor.name, target: event.target === document,
        bubbles: event.bubbles, cancelable: event.cancelable, detail: Object.hasOwn(event, 'detail'),
        math: document.querySelectorAll('.nativeMath').length, menus: document.querySelectorAll('[data-post-menu]').length,
        board: document.getElementById('watcher-context').dataset.board });
      if (hide) window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
    });
    document.addEventListener('4chanParsingDone', () => mainInitTrace.push({ parsed: true }));
  }, hideOnMain);
}
const mainSnapshot = board => ({ main: true, constructor: 'Event', target: true,
  bubbles: false, cancelable: false, detail: false, math: 0, menus: 0, board });

for (const [path, board, math] of [[pagePath, 'sci', true], ['/sci/', 'sci', true], ['/test/thread/1000001', 'test', false]]) {
  test(`MainInit precedes real parser and math startup on ${path}`, async ({ page }) => {
    await listenForMainInit(page); await page.goto(path);
    await expect.poll(() => page.evaluate(() => mainInitTrace.some(row => row.parsed))).toBe(true);
    if (math) await expect(page.locator('#m1000001 .nativeMath svg')).toHaveCount(2);
    const trace = await page.evaluate(() => mainInitTrace);
    expect(trace[0]).toEqual(mainSnapshot(board));
    expect(trace.filter(row => row.main)).toHaveLength(1);
    expect(trace.filter(row => row.mathWorker).length > 0).toBe(math);
    await page.evaluate(() => {
      document.dispatchEvent(new Event('4chanSettingsSaved'));
      window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
      window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
    });
    if (math) await expect(page.locator('#m1000001 .nativeMath svg')).toHaveCount(2);
    expect(await page.evaluate(() => mainInitTrace.filter(row => row.main).length)).toBe(1);
    expect(await page.evaluate(() => mainInitTrace.filter(row => row.parsed).length)).toBe(trace.filter(row => row.parsed).length);
  });
}

test('disabled extension still initializes once before independent board math, without ParsingDone', async ({ page }) => {
  await listenForMainInit(page);
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await page.goto(pagePath); await expect(page.locator('#m1000001 .nativeMath svg')).toHaveCount(2);
  const trace = await page.evaluate(() => mainInitTrace);
  expect(trace[0]).toEqual(mainSnapshot('sci')); expect(trace.filter(row => row.main)).toHaveLength(1);
  expect(trace.filter(row => row.parsed)).toHaveLength(0);
  await page.goto('/sci/catalog');
  await expect.poll(() => page.evaluate(() => mainInitTrace.filter(row => row.main).length)).toBe(1);
  expect(await page.evaluate(() => mainInitTrace)).toEqual([mainSnapshot('sci')]);
});

for (const interruption of ['restore-before-import', 'restore-after-import', 'main-listener', 'terminal']) {
  test(`delayed math import respects bootstrap interruption: ${interruption}`, async ({ page }) => {
    await listenForMainInit(page, interruption === 'main-listener');
    let release, requested;
    const held = new Promise(resolve => { release = resolve; });
    const started = new Promise(resolve => { requested = resolve; });
    await page.route(`**${modulePath}`, async route => { requested(); await held; await route.continue(); });
    try {
      await page.goto(pagePath, { waitUntil: 'domcontentloaded' }); await started;
      expect(await page.evaluate(() => mainInitTrace)).toEqual([mainSnapshot('sci')]);
      if (interruption !== 'main-listener') await page.evaluate(() => {
        window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
      });
      if (interruption === 'restore-before-import') await page.evaluate(() => {
        window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
      });
      if (interruption === 'terminal') await page.evaluate(() => {
        window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: false }));
      });
      release();
      // Observe the same module's evaluation without calling its page factory.
      await page.evaluate(path => import(path).then(() => true), modulePath);
      if (interruption !== 'restore-before-import') {
        await expect(page.locator('.nativeMath')).toHaveCount(0);
        await expect(page.locator('[data-post-menu]')).toHaveCount(0);
        expect(await page.evaluate(() => mainInitTrace)).toEqual([mainSnapshot('sci')]);
        await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
      }
      if (interruption === 'terminal') {
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
        expect(await page.evaluate(() => mainInitTrace)).toEqual([mainSnapshot('sci')]);
        await expect(page.locator('.nativeMath')).toHaveCount(0);
      } else {
        await expect(page.locator('#m1000001 .nativeMath svg')).toHaveCount(2);
        await expect.poll(() => page.evaluate(() => mainInitTrace.filter(row => row.parsed).length)).toBe(1);
        expect(await page.evaluate(() => mainInitTrace.filter(row => row.main).length)).toBe(1);
        await expect(page.locator('#threadWatcher')).toHaveCount(1);
        await page.locator('#pi1000001 a[title="Reply to this post"]').click();
        await expect(page.getByRole('button', { name: 'Preview TeX equations' })).toHaveCount(1);
      }
    } finally { release(); }
  });
}

test('failed math import keeps MainInit and ordinary initial parsing usable', async ({ page }) => {
  await listenForMainInit(page); await page.route(`**${modulePath}`, route => route.abort());
  await page.goto(pagePath);
  await expect.poll(() => page.evaluate(() => mainInitTrace.filter(row => row.parsed).length)).toBe(1);
  expect(await page.evaluate(() => mainInitTrace)).toEqual([mainSnapshot('sci'), { parsed: true }]);
  await expect(page.locator('#m1000001')).toContainText('[math]');
  await expect(page.locator('.nativeMath')).toHaveCount(0);
});

// Local safe-scanner behavior only. The supplied source configures delimiters
// but does not include the remote historical renderer's nested-tag semantics.
test('nested and crossed delimiters render nonrecursively while all source projections stay literal', async ({ page }) => {
  await page.addInitScript(() => { window.IntersectionObserver = undefined; });
  await page.goto('/sci/thread/1000002');
  const cases = [
    ['[math]a[math]b[/math]c[/math]', 'a[math]b', false, 'c[/math]'],
    ['[eqn]a[eqn]b[/eqn]c[/eqn]', 'a[eqn]b', true, 'c[/eqn]'],
    ['[math]a[eqn]b[/eqn]c[/math]', 'a[eqn]b[/eqn]c', false, ''],
    ['[eqn]a[math]b[/math]c[/eqn]', 'a[math]b[/math]c', true, ''],
    ['[math]a[eqn]b[/math]c[/eqn]', 'a[eqn]b', false, 'c[/eqn]'],
    ['[eqn]a[math]b[/eqn]c[/math]', 'a[math]b', true, 'c[/math]'],
    ['[math]a[eqn]b[/eqn]', 'b', true, '[math]a'],
  ];
  for (const [index, [input, tex, display, literal]] of cases.entries()) {
    const message = await addMessage(page, input, `nested-math-${index}`);
    await expect(message.locator('.nativeMath > svg')).toHaveCount(1);
    await expect(message.locator('.nativeMath')).toHaveAttribute('aria-label', tex);
    await expect(message.locator('.displayMath')).toHaveCount(display ? 1 : 0);
    expect(await message.textContent()).toBe(literal);
    const projections = await message.evaluate(async (node, path) => {
      const { projection } = (await import(path)).pageNativeMath();
      return [projection.text(node), projection.html(node), projection.clone(node).textContent];
    }, modulePath);
    expect(projections).toEqual([input, input, input]);
  }
  // Releasing ownership must restore the complete source, including unmatched
  // outer closers, rather than a reconstructed or recursively stripped form.
  await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
  for (const [index, [input]] of cases.entries()) {
    await expect(page.locator(`#nested-math-${index}`)).toHaveText(input);
    await expect(page.locator(`#nested-math-${index} svg`)).toHaveCount(0);
  }
  expect(await page.evaluate(() => window.mathCspViolations)).toEqual([]);
});

test('malformed and over-budget nested input stays literal, cannot execute HTML, and leaves the real worker usable', async ({ page }) => {
  const injectionRequests = [];
  page.on('request', request => {
    if (new URL(request.url()).pathname === '/math-injection') injectionRequests.push(request.url());
  });
  await page.addInitScript(() => {
    window.IntersectionObserver = undefined;
    window.nestedMathJobs = [];
    window.nestedMathExecuted = false;
    const Worker = window.Worker;
    window.Worker = class extends Worker {
      postMessage(job, ...rest) {
        window.nestedMathJobs.push({ tex: job.tex, display: job.display });
        return super.postMessage(job, ...rest);
      }
    };
  });
  await page.goto('/sci/thread/1000002');
  const deep = '[math]'.repeat(1000) + 'x' + '[/math]'.repeat(1000);
  const hostile = '[math][eqn]\\href{https://evil.invalid/x}{x}<img src="/math-injection" onerror="window.nestedMathExecuted=true">[/eqn][/math]';
  const inputs = [
    '[math]a[eqn]b', '[eqn]a[/math]',
    deep,
    '[math][math]x[/math][/math]'.repeat(257),
    '[math]x[/math]'.padEnd(65537, ' '),
    hostile,
  ];
  // Exercise the released controller and real worker on a dedicated root
  // outside the ordinary board's decoration ownership. Disabling extensions
  // alone does not isolate it: linkification still restores/unlinks messages.
  await page.evaluate(async ({ inputs, modulePath }) => {
    const { mountNativeMath, pageNativeMath } = await import(modulePath);
    const root = document.createElement('section');
    root.id = 'nested-math-owned-root';
    document.body.append(root);
    const controller = mountNativeMath({ root, projection: pageNativeMath().projection });
    if (!controller) throw new Error('Owned math controller was not admitted');
    const batch = document.createDocumentFragment();
    for (const [index, input] of [...inputs, '[math]z+1[/math]'].entries()) {
      const message = document.createElement('blockquote');
      message.className = 'postMessage';
      message.id = index === inputs.length ? 'nested-sentinel' : `literal-math-${index}`;
      message.textContent = input;
      batch.append(message);
    }
    root.append(batch);
  }, { inputs, modulePath });
  const sentinel = page.locator('#nested-sentinel');
  await expect(sentinel.locator('.nativeMath > svg')).toHaveCount(1);
  for (const [index, input] of inputs.entries()) {
    const message = page.locator(`#literal-math-${index}`);
    expect(await message.textContent()).toBe(input);
    await expect(message.locator('svg, img, script, iframe, style, a, foreignObject')).toHaveCount(0);
    if (input.length > 65536) {
      // HTML/clone readers have their own 65,536-character text-node bound.
      // Their refusal must leave the larger literal DOM/text projection intact.
      const bounded = await message.evaluate(async (node, path) => {
        const { projection } = (await import(path)).pageNativeMath();
        const errors = ['html', 'clone'].map(method => {
          try { projection[method](node); return null; }
          catch (error) { return { name: error.name, message: error.message }; }
        });
        return { text: projection.text(node), errors, literal: node.textContent };
      }, modulePath);
      expect(bounded).toEqual({ text: input, literal: input, errors: [
        { name: 'RangeError', message: 'comment-text' },
        { name: 'RangeError', message: 'comment-text' },
      ] });
    } else {
      const projections = await message.evaluate(async (node, path) => {
        const { projection } = (await import(path)).pageNativeMath();
        return [projection.text(node), projection.html(node), projection.clone(node).textContent];
      }, modulePath);
      const escaped = input.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
      expect(projections).toEqual([input, escaped, input]);
    }
  }
  // Deep delimiter text is one bounded job, not 1,000 recursive jobs. The
  // worker's 4,096-character TeX limit rejects it. Projection text-node and
  // scanner span-count overflows queue none.
  expect(await page.evaluate(() => window.nestedMathJobs)).toEqual([
    { tex: '[math]'.repeat(999) + 'x', display: false },
    { tex: hostile.slice('[math]'.length, -'[/math]'.length), display: false },
    { tex: 'z+1', display: false },
  ]);
  expect(injectionRequests).toEqual([]);
  expect(await page.evaluate(() => window.nestedMathExecuted)).toBe(false);
  expect(await page.evaluate(() => window.mathCspViolations)).toEqual([]);
});
