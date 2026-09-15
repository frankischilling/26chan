import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

test('filter settlement follows bounded replacement work', async t => {
  const bundle = await readFile(new URL('../../apps/public/static/native-filter.v1.js', import.meta.url), 'utf8');
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup() {
      const context = await browser.newContext();
      const page = await context.newPage();
      await page.route('https://settlement.example/**', async route => {
        if (new URL(route.request().url()).pathname === '/static/native-filter.v1.js') {
          await route.fulfill({ contentType: 'text/javascript', body: bundle });
        } else await route.fulfill({ contentType: 'text/html', body: `<!doctype html><main class="board"><section class="thread" id="t100">
          <article class="postContainer" id="pc101"><div class="post reply" id="p101"><div class="postInfo"><span class="name">Anonymous</span></div><blockquote class="postMessage" id="m101">Match</blockquote></div></article>
          </section></main>` });
      });
      await page.goto('https://settlement.example/demo/thread/100');
      await page.evaluate(async () => {
        const { mountNativeFilters } = await import('/static/native-filter.v1.js');
        window.jobs = []; window.ignoreAbort = false; window.result = 'not-started';
        window.filterConfig = { filter: true };
        window.filterRaw = JSON.stringify([{ type: 2, pattern: 'Match', boards: 'demo', active: true, color: '#ff0000' }]);
        window.filters = mountNativeFilters({
          board: 'demo', threadId: '100', settings: () => ({ ...window.filterConfig }),
          read: () => window.filterRaw,
          save() {}, changed() {}, getTracked: () => new Set(),
          match(_rules, _board, _posts, { signal }) {
            return new Promise(resolve => {
              const job = { signal, resolve };
              window.jobs.push(job);
              if (!window.ignoreAbort) signal.addEventListener('abort', () => resolve({ status: 'cancelled' }), { once: true });
            });
          },
        });
        window.begin = () => {
          window.cycle = new AbortController(); window.result = 'pending';
          window.filters.refreshSettled(window.cycle.signal).then(value => { window.result = value; });
        };
        window.release = () => window.jobs.at(-1).resolve({ status: 'ok', matches: [{ id: '101', filter: 0 }] });
      });
      return { context, page };
    }

    for (const replacement of ['observer', 'explicit refresh']) {
      await t.test(`${replacement} cannot settle the caller before its replacement match`, async () => {
        const { context, page } = await setup();
        try {
          await page.evaluate(() => window.begin());
          await page.waitForFunction(() => window.jobs.length === 1);
          await page.evaluate(replacement => {
            if (replacement === 'observer') document.getElementById('m101').append(' after decoration');
            else window.filters.refresh();
          }, replacement);
          await page.waitForFunction(() => window.jobs.length === 2);
          assert.equal(await page.evaluate(() => window.jobs[0].signal.aborted), true);
          assert.equal(await page.evaluate(() => window.result), 'pending');
          await page.evaluate(() => window.release());
          await page.waitForFunction(() => window.result !== 'pending');
          assert.deepEqual(await page.evaluate(() => ({
            result: window.result,
            highlighted: document.getElementById('p101').classList.contains('filter-hl'),
            notice: document.querySelector('.nativeFilterNotice').textContent,
          })), { result: true, highlighted: true, notice: '' });
        } finally { await context.close(); }
      });
    }

    for (const change of ['rules', 'disabled setting']) {
      await t.test(`a ${change} change visible before its storage event cannot falsely settle an unapplied pass`, async () => {
        const { context, page } = await setup();
        try {
          await page.evaluate(() => window.begin());
          await page.waitForFunction(() => window.jobs.length === 1);
          await page.evaluate(change => {
            if (change === 'rules') {
              const rules = JSON.parse(window.filterRaw); rules[0].color = '#0000ff';
              window.filterRaw = JSON.stringify(rules);
            } else window.filterConfig.filter = false;
            // Deliberately leave the notification callback undelivered.
            window.release();
          }, change);
          if (change === 'rules') {
            await page.waitForFunction(() => window.jobs.length === 2 || window.result !== 'pending');
            assert.equal(await page.evaluate(() => window.result), 'pending');
            assert.equal(await page.evaluate(() => window.jobs.length), 2);
            await page.evaluate(() => window.release());
          }
          await page.waitForFunction(() => window.result !== 'pending');
          assert.deepEqual(await page.evaluate(() => ({
            result: window.result,
            highlighted: document.getElementById('p101').classList.contains('filter-hl'),
            notice: document.querySelector('.nativeFilterNotice').textContent,
          })), { result: true, highlighted: change === 'rules', notice: '' });
          if (change === 'rules') assert.ok((await page.locator('#p101').evaluate(node => node.style.boxShadow)).includes('rgb(0, 0, 255)'));
        } finally { await context.close(); }
      });
    }

    await t.test('cycle cancellation and page exit settle without a worker response, and a restored page can retry', async () => {
      const { context, page } = await setup();
      try {
        await page.evaluate(() => { window.ignoreAbort = true; window.begin(); });
        await page.waitForFunction(() => window.jobs.length === 1);
        await page.evaluate(() => {
          window.cycle.abort(); window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
        });
        await page.waitForFunction(() => window.result === false);
        assert.equal(await page.evaluate(() => window.jobs[0].signal.aborted), true);
        await page.evaluate(() => {
          window.jobs[0].resolve({ status: 'ok', matches: [{ id: '101', filter: 0 }] });
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
        });
        assert.equal(await page.locator('#p101').getAttribute('class'), 'post reply');
        await page.evaluate(() => { window.ignoreAbort = false; window.begin(); });
        await page.waitForFunction(() => window.jobs.length === 2);
        await page.evaluate(() => window.release());
        await page.waitForFunction(() => window.result === true);
      } finally { await context.close(); }
    });

    await t.test('the total deadline releases a wait even when the match ignores abort', async () => {
      const { context, page } = await setup();
      try {
        const time = new Date('2026-09-15T15:00:00Z');
        await page.clock.install({ time }); await page.clock.pauseAt(time);
        await page.evaluate(() => { window.ignoreAbort = true; window.begin(); });
        await page.waitForFunction(() => window.jobs.length === 1);
        await page.clock.runFor(60001);
        assert.equal(await page.evaluate(() => window.result), false);
        assert.equal(await page.evaluate(() => window.jobs[0].signal.aborted), true);
        await page.evaluate(() => {
          window.jobs[0].resolve({ status: 'cancelled' }); window.ignoreAbort = false; window.begin();
        });
        await page.waitForFunction(() => window.jobs.length === 2);
        await page.evaluate(() => window.release());
        await page.waitForFunction(() => window.result === true);
      } finally { await context.close(); }
    });

    await t.test('continuous replacement cannot extend a caller beyond 64 filter generations', async () => {
      const { context, page } = await setup();
      try {
        await page.evaluate(() => window.begin());
        await page.waitForFunction(() => window.jobs.length === 1);
        const result = await page.evaluate(async () => {
          for (let index = 0; index < 70 && window.result === 'pending'; index++) {
            window.filters.refresh();
            await new Promise(resolve => setTimeout(resolve, 0));
          }
          return { result: window.result, jobs: window.jobs.length };
        });
        assert.equal(result.result, false);
        assert.ok(result.jobs >= 64 && result.jobs <= 65);
        // The cap ends this caller's wait; the independent current page filter
        // remains usable and can finish normally.
        await page.evaluate(() => window.release());
        await page.waitForFunction(() => document.getElementById('p101').classList.contains('filter-hl'));
      } finally { await context.close(); }
    });
  } finally { await browser.close(); }
});
