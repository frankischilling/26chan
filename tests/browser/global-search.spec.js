import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';

const origin = 'http://127.0.0.1:3000';

for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test(`global search renders persisted scoped results under CSP at ${viewport.width}px`, async ({ browser }) => {
    const context = await browser.newContext({ viewport });
    const page = await context.newPage();
    const marker = `OwnedSearch${randomUUID().replaceAll('-', '')}`;
    const password = `delete-${randomUUID()}`;
    const threads = [];
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(() => {
      window.searchPolicyViolations = [];
      document.addEventListener('securitypolicyviolation', event => {
        window.searchPolicyViolations.push(`${event.effectiveDirective}: ${event.blockedURI}`);
      });
    });
    async function post(subject, comment, thread = '0') {
      const response = await withPostingHistory(() => context.request.post(`${origin}/fixture/imgboard.php`, {
        headers: { Origin: origin, Accept: 'application/json' },
        form: { mode: 'regist', pwd: password, sub: subject, com: comment, resto: thread },
        maxRedirects: 0,
      }));
      expect(response.status(), await response.text()).toBe(200);
      const result = await response.json();
      expect(result.error).toBeUndefined();
      expect(Number.isSafeInteger(result.pid)).toBe(true);
      const id = String(result.pid);
      if (thread === '0') threads.push(id);
      return id;
    }
    try {
      const first = await post(`${marker} first`, 'Original thread context');
      const reply = await post('', `${'a'.repeat(1300)} ${marker} <img src=x onerror=alert(1)>`, first);
      const second = await post(`${marker} second`, 'Separate matching thread');
      const response = await page.goto(`/globalsearch.php#/${marker}/fixture`);
      expect(response.status()).toBe(200);
      expect(response.headers()['content-security-policy']).toContain("style-src 'self'");
      expect(response.headers()['content-security-policy']).toContain('/static/global-search.v1.js');
      await expect(page.locator('#js-sf-bf')).toHaveValue('fixture');
      await expect(page.locator('#js-sf-bf option[value="j"]')).toHaveCount(0);
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(2);
      await expect(page.locator(`#t${first} #p${reply} .postMessage`)).toContainText(marker);
      await expect(page.locator(`#t${first} #p${reply} .postMessage`)).toContainText('<img src=x onerror=alert(1)>');
      await expect(page.locator(`#t${first} #p${reply} .postMessage img`)).toHaveCount(0);
      await expect(page.locator(`#p${reply} .postNum > a`).first()).toHaveAttribute('href', `/fixture/thread/${first}#p${reply}`);
      const excerpt = await page.locator(`#p${reply} .postMessage`).textContent();
      expect(excerpt.length).toBeLessThanOrEqual(1024);
      expect(excerpt).not.toContain('a'.repeat(1025));

      await page.locator('#js-sf-bf').selectOption('');
      await page.locator('#js-sf-btn').click();
      await expect(page).toHaveURL(new RegExp(`/globalsearch\\.php#/${marker}$`));
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(2);

      await page.locator('#js-sf-qf').fill(`${marker} second`);
      await page.locator('#js-sf-btn').click();
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(1);
      await expect(page.locator(`#t${second}`)).toBeVisible();
      await page.evaluate(hash => { location.hash = hash; }, `#/${marker}`);
      await expect(page.locator('#js-sf-qf')).toHaveValue(marker);
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(2);

      await withDeletionQuota(async () => {
        const removed = await context.request.post(`${origin}/fixture/delete`, {
          headers: { Origin: origin }, form: { no: first, password }, maxRedirects: 0,
        });
        expect(removed.status()).toBe(303);
      });
      threads.splice(threads.indexOf(first), 1);
      await page.reload();
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(1);
      await expect(page.locator(`#t${first}`)).toHaveCount(0);
      await expect(page.locator(`#t${second}`)).toBeVisible();
      const privateResult = await context.request.get(`/search/api?q=${marker}&b=j&o=0`);
      expect(privateResult.status()).toBe(200);
      expect(await privateResult.json()).toEqual({ threads: [], offset: 0, nhits: 0 });
      expect(await page.evaluate(() => window.searchPolicyViolations)).toEqual([]);
      expect(errors).toEqual([]);
      // Follow the actual shared OP fragment after worker/main-thread validation.
      // The exact source context comes from the persisted subject, not the URL.
      const jsonResponse = await context.request.get(`${origin}/fixture/thread/${second}.json`);
      expect(jsonResponse.status()).toBe(200);
      const op = (await jsonResponse.json()).posts[0];
      // The 43-byte marker fits; adding the next word exceeds the source 50-byte budget.
      expect(op.semantic_url).toBe(marker.toLowerCase());
      const replyHref = `/fixture/thread/${second}/${op.semantic_url}`;
      // Check the rendered href even when the source mobile layout hides its header.
      const replyLink = page.locator(`#pi${second}`).getByRole('link', { name: 'Reply', exact: true, includeHidden: true });
      await expect(replyLink).toHaveAttribute('href', replyHref);
      // The desktop header is hidden by the source mobile layout; the same
      // server link remains testable through a normal click at desktop width.
      if (viewport.width < 480) await page.setViewportSize({ width: 1280, height: 900 });
      await replyLink.click();
      await expect(page).toHaveURL(`${origin}${replyHref}`);
      await expect(page.locator(`#t${second} #p${second}`)).toBeVisible();
      await expect(page.locator('form.postEditor input[name="resto"]')).toHaveValue(second);
    } finally {
      for (const thread of threads) {
        await withDeletionQuota(async () => {
          const response = await context.request.post(`${origin}/fixture/delete`, {
            headers: { Origin: origin }, form: { no: thread, password }, maxRedirects: 0,
          });
          expect(response.status()).toBe(303);
        });
      }
      await context.close();
    }
  });

  test(`global search paginates persisted threads and handles empty or invalid searches at ${viewport.width}px`, async ({ browser }) => {
    const context = await browser.newContext({ viewport });
    const page = await context.newPage();
    const marker = `OwnedPages${randomUUID().replaceAll('-', '')}`;
    const password = `delete-${randomUUID()}`;
    const threads = [];
    try {
      for (let index = 0; index < 12; index++) {
        const response = await withPostingHistory(() => context.request.post(`${origin}/fixture/post`, {
          headers: { Origin: origin, Accept: 'application/json' },
          form: { pwd: password, sub: `${marker} ${index}`, com: 'Owned pagination fixture', resto: '0' },
        }));
        expect(response.status(), await response.text()).toBe(200);
        threads.push(String((await response.json()).pid));
      }
      await page.goto(`/globalsearch.php#/${marker}/fixture`);
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(10);
      await expect(page.locator('#js-sf-pl .pages')).toHaveText('Page 1 / 2');
      const firstPage = await page.locator('#js-sf-results .thread').evaluateAll(nodes => nodes.map(node => node.id));
      await page.locator('#js-sf-pl').getByRole(viewport.width < 480 ? 'link' : 'button', { name: 'Next', exact: true }).click();
      await expect(page).toHaveURL(new RegExp(`/globalsearch\\.php#/${marker}/fixture/2$`));
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(2);
      await expect(page.locator('#js-sf-pl .pages')).toHaveText('Page 2 / 2');
      const secondPage = await page.locator('#js-sf-results .thread').evaluateAll(nodes => nodes.map(node => node.id));
      expect(new Set([...firstPage, ...secondPage])).toEqual(new Set(threads.map(id => `t${id}`)));
      await page.reload();
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(2);
      await page.locator('#js-sf-pl').getByRole(viewport.width < 480 ? 'link' : 'button', { name: 'Previous', exact: true }).click();
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(10);

      await page.locator('#js-sf-qf').fill(`${marker} missing`);
      await page.locator('#js-sf-btn').click();
      await expect(page.locator('#js-sf-status')).toHaveText('Nothing found.');
      await expect(page.locator('#js-sf-pl')).toHaveCount(0);
      await page.evaluate(() => { location.hash = '#/%E0%A4%A'; });
      await expect(page.locator('#js-sf-status')).toHaveText('Something went wrong.');
      await expect(page.locator('#js-sf-btn')).toBeEnabled();
      for (const [query, status] of [['q=', 422], ['q=owned&o=1', 422], ['q=owned&o=100', 422], ['q=owned&unknown=1', 400]]) {
        expect((await context.request.get(`/search/api?${query}`)).status()).toBe(status);
      }
    } finally {
      try {
        for (const thread of threads) {
          await withDeletionQuota(async () => {
            expect((await context.request.post(`${origin}/fixture/delete`, {
              headers: { Origin: origin }, form: { no: thread, password }, maxRedirects: 0,
            })).status()).toBe(303);
          });
        }
      } finally { await context.close(); }
    }
  });
}

// These are real page/API navigations. Expected hash parsing comes from the
// unchanged pinned client methods, not a second implementation of the grammar.
for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test(`direct hash boundaries and ToInt32 pages match source at ${viewport.width}px`, async ({ browser }) => {
    const { sourceSearch } = await import('./helpers/global-search-source.mjs');
    const { captureSearchResponses } = await import('./helpers/global-search-response.mjs');
    const context = await browser.newContext({ viewport });
    await context.addInitScript(captureSearchResponses);
    const page = await context.newPage();
    const marker = `NoSearchHit${randomUUID().replaceAll('-', '')}`;
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    const cases = [
      `#${marker}`, `#/${marker}/fixture/0x2`, `#/${marker}/fixture/2junk`,
      `#/${marker}/fixture/2.9`, `#/${marker}/fixture/1e1`,
      `#/${marker}/fixture/Infinity`, `#/${marker}/fixture/-1`,
      `#/${marker}/fixture/4294967298`, `#/${marker}/fixture/2147483648`,
      `#/${marker}/all/2`, `#/${marker}/missing/2`,
      `#/${marker}%2F%252F%2B/fixture/2`, '#/%E0%A4%A',
      ...[510, 511, 512].map(length => `#/${marker.padEnd(length, 'x')}`),
      `#/${'%41'.repeat(170)}`, `#/${'%41'.repeat(171)}`,
    ];
    try {
      for (const hash of cases) {
        await page.goto('about:blank');
        // URL serialization is browser behavior; source receives location.hash.
        const url = new URL(`/globalsearch.php${hash}`, origin);
        const expected = sourceSearch(url.hash, ['fixture']);
        const requests = [];
        const record = request => { if (new URL(request.url()).pathname === '/search/api') requests.push(new URL(request.url())); };
        page.on('request', record);
        const responsePromise = expected?.query
          ? page.waitForResponse(response => new URL(response.url()).pathname === '/search/api') : null;
        await page.goto(url.href);
        if (responsePromise) {
          const response = await responsePromise;
          expect(response.status()).toBe(200);
          const captured = await page.evaluate(() => Promise.all(window.ownedSearchResponses));
          expect(captured).toHaveLength(1);
          expect(captured[0].status).toBe(200);
          expect(captured[0].type).toMatch(/^application\/json\b/);
          expect(captured[0].failed).toBeUndefined();
          expect(captured[0].result.offset).toBe(expected.offset);
          await expect(page.locator('#js-sf-btn')).toBeEnabled();
          expect(requests).toHaveLength(1);
          expect(requests[0].searchParams.get('q')).toBe(expected.query);
          expect(requests[0].searchParams.get('b') || '').toBe(expected.board);
          expect(Number(requests[0].searchParams.get('o') || 0)).toBe(expected.offset);
          expect(await page.locator('#js-sf-qf').inputValue()).toBe(expected.query);
          expect(await page.locator('#js-sf-bf').inputValue()).toBe(expected.board);
        } else {
          expect(requests).toHaveLength(0);
          expect(await page.evaluate(() => window.ownedSearchResponses.length)).toBe(0);
          expect(await page.locator('#js-sf-qf').inputValue()).toBe('');
          if (expected === null) await expect(page.locator('#js-sf-status')).toHaveText('Something went wrong.');
          else await expect(page.locator('#js-sf-results')).toBeEmpty();
        }
        page.off('request', record);
      }
      expect(errors).toEqual([]);
    } finally { await context.close(); }
  });
}

// Observe the real fetch signal/promise without replacing either. The mutation
// log catches stale results even if another render would remove them later.
function observeSearchLifecycle() {
  const fetch = window.fetch;
  window.ownedSearchLifecycle = [];
  window.ownedSearchCommits = [];
  window.fetch = function (input, options) {
    const url = new URL(typeof input === 'string' || input instanceof URL ? input : input.url, location.href);
    let entry;
    if (url.origin === location.origin && url.pathname === '/search/api') {
      const signal = options?.signal ?? input?.signal;
      entry = { query: url.searchParams.get('q'), aborted: signal?.aborted === true, settled: false, error: null };
      window.ownedSearchLifecycle.push(entry);
      signal?.addEventListener('abort', () => { entry.aborted = true; }, { once: true });
    }
    const pending = Reflect.apply(fetch, this, [input, options]);
    if (entry) pending.then(() => { entry.settled = true; }, error => {
      entry.settled = true; entry.error = error.name;
    });
    return pending;
  };
  new MutationObserver(() => {
    window.ownedSearchCommits.push([...document.querySelectorAll('#js-sf-results .thread')].map(node => node.id));
  }).observe(document, { childList: true, subtree: true });
}

// Hold a successful, persisted API response after the server has generated it.
// Only delivery timing changes; no post fragments or response fields are made up.
async function holdSearchResponse(page, query) {
  let release, captured, failed, complete, failure, active = false, closed = false;
  const gate = new Promise(resolve => { release = resolve; });
  const ready = new Promise((resolve, reject) => { captured = resolve; failed = reject; });
  // Own early rejection even if page setup fails before the caller awaits ready.
  // Return the original promise so its awaited rejection still fails the test.
  void ready.catch(() => {});
  const finished = new Promise(resolve => { complete = resolve; });
  const matches = url => url.pathname === '/search/api' && url.searchParams.get('q') === query;
  const handler = async route => {
    active = true;
    try {
      const response = await route.fetch({ maxRedirects: 0, maxRetries: 0 });
      const body = await response.text();
      captured({ status: response.status(), body });
      await gate;
      await route.fulfill({ response, body }).catch(error => {
        // Playwright can reject fulfillment after the browser has cancelled.
        // Any failure without a failed browser request remains a test failure.
        if (!route.request().failure()) throw error;
      });
    } catch (error) { failure = error; failed(error); }
    finally { complete(); }
  };
  await page.route(matches, handler, { times: 1 });
  return { ready, async close() {
    if (closed) return;
    closed = true; release();
    await page.unroute(matches, handler);
    if (active) await finished;
    if (failure) throw failure;
  } };
}

for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test(`global search cancels pending responses and recovers from transport failure at ${viewport.width}px`, async ({ browser }) => {
    const context = await browser.newContext({ viewport });
    await context.addInitScript(observeSearchLifecycle);
    const page = await context.newPage();
    const marker = `OwnedLifecycle${randomUUID().replaceAll('-', '')}`;
    const staleQuery = `${marker} stale`, currentQuery = `${marker} current`;
    const password = `delete-${randomUUID()}`;
    const threads = [];
    const errors = [];
    const heldResponses = [];
    const cleanupErrors = [];
    let testFailure;
    page.on('pageerror', error => errors.push(error.message));
    const hashFor = query => `#/${encodeURIComponent(query)}/fixture`;
    async function createThread(subject) {
      const response = await withPostingHistory(() => context.request.post(`${origin}/fixture/post`, {
        headers: { Origin: origin, Accept: 'application/json' },
        form: { pwd: password, sub: subject, com: 'Owned search lifecycle fixture', resto: '0' },
      }));
      expect(response.status(), await response.text()).toBe(200);
      const result = await response.json();
      expect(result.error).toBeUndefined();
      expect(Number.isSafeInteger(result.pid)).toBe(true);
      const id = String(result.pid);
      threads.push(id);
      return id;
    }
    async function expectCurrent(id) {
      await expect(page.locator('#js-sf-btn')).toBeEnabled();
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(1);
      await expect(page.locator(`#js-sf-results #t${id}`)).toBeVisible();
      await expect(page.locator('#js-sf-qf')).toHaveValue(currentQuery);
      await expect(page.locator('#js-sf-bf')).toHaveValue('fixture');
      await expect(page).toHaveURL(`${origin}/globalsearch.php${hashFor(currentQuery)}`);
    }
    try {
      const staleId = await createThread(staleQuery);
      const currentId = await createThread(currentQuery);
      await page.goto('/globalsearch.php');
      for (const replacement of [
        { hash: hashFor(currentQuery), kind: 'current' },
        { hash: '', kind: 'empty' },
        { hash: '#/%E0%A4%A', kind: 'malformed' },
        { hash: `#/${'x'.repeat(511)}`, kind: 'empty' },
      ]) {
        const held = await holdSearchResponse(page, staleQuery);
        heldResponses.push(held);
        await page.evaluate(hash => { location.hash = hash; }, hashFor(staleQuery));
        const captured = await held.ready;
        expect(captured.status).toBe(200);
        const data = JSON.parse(captured.body);
        expect(data.offset).toBe(0);
        expect(data.nhits).toBe(1);
        expect(data.threads.map(thread => thread.thread)).toEqual([staleId]);
        await expect(page.locator('#js-sf-status')).toHaveText('Searching…');
        await expect(page.locator('#js-sf-btn')).toBeDisabled();
        await expect(page.locator('#js-sf-results .thread')).toHaveCount(0);
        expect(await page.evaluate(query => window.ownedSearchLifecycle.filter(entry => entry.query === query).at(-1), staleQuery))
          .toEqual({ query: staleQuery, aborted: false, settled: false, error: null });

        await page.evaluate(hash => { location.hash = hash; }, replacement.hash);
        await expect.poll(() => page.evaluate(query => window.ownedSearchLifecycle.filter(entry => entry.query === query).at(-1), staleQuery))
          .toEqual({ query: staleQuery, aborted: true, settled: true, error: 'AbortError' });
        await expect(page.locator('#js-sf-btn')).toBeEnabled();
        if (replacement.kind === 'current') await expectCurrent(currentId);
        else if (replacement.kind === 'malformed') await expect(page.locator('#js-sf-status')).toHaveText('Something went wrong.');
        else await expect(page.locator('#js-sf-results')).toBeEmpty();

        // Release the older successful response after the new state has settled.
        await held.close();
        if (replacement.kind === 'current') await expectCurrent(currentId);
        else if (replacement.kind === 'malformed') await expect(page.locator('#js-sf-status')).toHaveText('Something went wrong.');
        else await expect(page.locator('#js-sf-results')).toBeEmpty();
        await expect(page.locator(`#t${staleId}`)).toHaveCount(0);
        expect(await page.evaluate(id => window.ownedSearchCommits.some(ids => ids.includes(`t${id}`)), staleId)).toBe(false);
      }

      // A genuine failed browser request must restore the form and permit a
      // subsequent search through the unmodified server endpoint.
      const failedQuery = `${marker} transport`;
      await page.route(url => url.pathname === '/search/api' && url.searchParams.get('q') === failedQuery,
        route => route.abort('failed'), { times: 1 });
      await page.evaluate(hash => { location.hash = hash; }, hashFor(failedQuery));
      await expect(page.locator('#js-sf-status')).toHaveText('Connection error.');
      await expect(page.locator('#js-sf-btn')).toBeEnabled();
      await expect(page.locator('#js-sf-pl')).toHaveCount(0);
      await expect(page.locator('#js-sf-results .thread')).toHaveCount(0);
      await expect.poll(() => page.evaluate(query => window.ownedSearchLifecycle.find(entry => entry.query === query), failedQuery))
        .toEqual({ query: failedQuery, aborted: false, settled: true, error: 'TypeError' });
      await page.locator('#js-sf-qf').fill(currentQuery);
      await page.locator('#js-sf-btn').click();
      await expectCurrent(currentId);
      expect(await page.evaluate(id => window.ownedSearchCommits.some(ids => ids.includes(`t${id}`)), staleId)).toBe(false);
      expect(errors).toEqual([]);
    } catch (error) { testFailure = error; }
    finally {
      for (const held of heldResponses) {
        try { await held.close(); }
        catch (error) { cleanupErrors.push(error); }
      }
      for (const thread of threads) {
        try {
          await withDeletionQuota(async () => {
            expect((await context.request.post(`${origin}/fixture/delete`, {
              headers: { Origin: origin }, form: { no: thread, password }, maxRedirects: 0,
            })).status()).toBe(303);
          });
        } catch (error) { cleanupErrors.push(error); }
      }
      try { await context.close(); }
      catch (error) { cleanupErrors.push(error); }
    }
    if (cleanupErrors.length) {
      throw new AggregateError(testFailure ? [testFailure, ...cleanupErrors] : cleanupErrors,
        'Global Search lifecycle test or owned-fixture cleanup failed', { cause: testFailure });
    }
    if (testFailure) throw testFailure;
  });
}
