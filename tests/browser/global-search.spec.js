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
