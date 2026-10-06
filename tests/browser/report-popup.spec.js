import { withDeletionQuota, withPostingHistory, withReportCatalog } from './helpers/deletion-quota-fixture.js';
import { test as base, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-report-popup-password';
    const write = form => withPostingHistory(() => request.post('/demo/post', {
      headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password },
    }));
    const created = await write({ resto: '0', sub: 'Owned report popup', com: 'Owned report OP' });
    expect(created.status()).toBe(303);
    const id = created.headers().location.match(/#p(\d+)$/)[1];
    try {
      const response = await write({ resto: id, com: 'Owned report reply' });
      expect(response.status()).toBe(303);
      await use({ id, reply: response.headers().location.match(/#p(\d+)$/)[1], url: `/demo/thread/${id}` });
    } finally {
      await withDeletionQuota(async () => {
        const removed = await request.post('/demo/delete', { headers: { Origin: origin }, maxRedirects: 0,
          form: { no: id, password } });
        expect(removed.status()).toBe(303);
      });
    }
  },
});
async function menu(page, id, action = 'Report post') {
  await page.getByRole('button', { name: `Post menu for post ${id}`, exact: true }).click();
  await page.getByRole('menuitem', { name: action, exact: true }).click();
}
async function openReport(page, id) {
  const opened = page.waitForEvent('popup'); await menu(page, id);
  const popup = await opened; await popup.waitForLoadState();
  await expect(popup).toHaveURL(`${origin}/demo/imgboard.php?mode=report&no=${id}`);
  await expect(popup.locator('#report-popup-close')).toBeVisible();
  return popup;
}
async function submit(popup) {
  await popup.locator('#reason').fill('Owned report popup browser test');
  await popup.locator('#report-submit').click();
  await expect(popup.locator('#report-popup-context')).toHaveAttribute('data-result', 'success');
}
const hidden = (page, id) => page.locator(`#m${id}`);

test('native report popup commits, hides only its registered reply, and closes after the source delay', async ({ page, owned }) => {
  await page.goto(owned.url);
  const popup = await openReport(page, owned.reply);
  await popup.clock.install(); await popup.clock.pauseAt(new Date(Date.now() + 1000));
  await submit(popup);
  await expect(hidden(page, owned.reply)).toBeHidden(); await expect(hidden(page, owned.id)).toBeVisible();
  await popup.clock.fastForward(2999); expect(popup.isClosed()).toBe(false);
  const closed = popup.waitForEvent('close'); await popup.clock.fastForward(1); await closed;
  await expect(page).toHaveURL(`${origin}${owned.url}`);
});

test('Close and Escape cancel without success; validation errors do not hide or auto-close', async ({ page, owned }) => {
  await page.goto(owned.url);
  let popup = await openReport(page, owned.reply);
  let closed = popup.waitForEvent('close'); await popup.locator('#report-popup-close').click(); await closed;
  await expect(hidden(page, owned.reply)).toBeVisible();
  popup = await openReport(page, owned.reply);
  // Escape closes during keydown, so Chromium may destroy the input target
  // before Playwright receives its keyup acknowledgement. Prove delivery and
  // the intended popup-only closure rather than retrying or masking errors.
  await page.evaluate(() => { window.__reportEscapeObserved = false; });
  await popup.evaluate(() => document.addEventListener('keydown', event => {
    if (event.key === 'Escape' && event.isTrusted) opener.__reportEscapeObserved = true;
  }, { capture: true, once: true }));
  closed = popup.waitForEvent('close');
  try {
    await popup.keyboard.press('Escape');
  } catch (error) {
    if (!popup.isClosed() || page.isClosed() || !page.context().browser()?.isConnected()
        || !String(error).includes('Target page, context or browser has been closed')) throw error;
  }
  await closed;
  expect(await page.evaluate(() => window.__reportEscapeObserved)).toBe(true);
  expect(page.context().browser()?.isConnected()).toBe(true);
  await expect(hidden(page, owned.reply)).toBeVisible();
  popup = await openReport(page, owned.reply);
  await popup.clock.install();
  await popup.evaluate(() => { document.getElementById('reason').value = ''; document.getElementById('report-form').submit(); });
  await expect(popup.locator('#report-popup-context')).toHaveAttribute('data-result', 'error');
  await popup.clock.fastForward(5000); expect(popup.isClosed()).toBe(false);
  await expect(hidden(page, owned.reply)).toBeVisible(); await popup.close();
});

test('ordinary report tabs retain Return, never auto-close or navigate, and Close follows the return link', async ({ page, owned }) => {
  await page.goto(`/demo/imgboard.php?mode=report&no=${owned.reply}`);
  await page.clock.install(); await submit(page);
  await expect(page.locator('#report-popup-return')).toBeVisible();
  await page.clock.fastForward(5000); expect(page.isClosed()).toBe(false);
  await expect(page).toHaveURL(`${origin}/demo/report`);
  await page.locator('#report-popup-close').click(); await expect(page).toHaveURL(`${origin}/demo/`);
});

for (const blocked of ['null', 'throw']) test(`blocked popup ${blocked} falls back to canonical native GET`, async ({ page, owned }) => {
  await page.goto(owned.url);
  await page.evaluate(blocked => { window.open = () => { if (blocked === 'throw') throw new Error('blocked'); return null; }; }, blocked);
  await menu(page, owned.reply);
  await expect(page).toHaveURL(`${origin}/demo/imgboard.php?mode=report&no=${owned.reply}`);
  await expect(page.locator('#report-form')).toBeVisible();
});

test('receiver rejects foreign, unregistered, wrong-target, generic, malformed and replay messages', async ({ page, context, owned }) => {
  await page.goto(owned.url);
  await page.evaluate(() => {
    const open = window.open;
    window.open = (...args) => { window.openedReport = open.apply(window, args); return window.openedReport; };
  });
  const popup = await openReport(page, owned.reply);
  const stranger = await context.newPage(); await stranger.goto(owned.url);
  await page.evaluate(id => {
    for (const data of [`done-report-${id}-demo`, 'done-report', {}, `done-report-0${id}-demo`]) {
      window.dispatchEvent(new MessageEvent('message', { origin: location.origin, source: window, data }));
    }
    window.dispatchEvent(new MessageEvent('message', { origin: 'https://foreign.invalid', source: window.openedReport, data: `done-report-${id}-demo` }));
  }, owned.reply);
  await popup.evaluate(({ id, reply }) => {
    for (const data of ['done-report', `done-report-${id}-demo`, `done-report-${reply}-other`, `done-report-${reply}-demo-extra`, {}]) {
      opener.postMessage(data, location.origin);
    }
  }, owned);
  await expect(hidden(page, owned.reply)).toBeVisible();
  await popup.clock.install(); await popup.clock.pauseAt(new Date(Date.now() + 1000)); await submit(popup);
  await expect(hidden(page, owned.reply)).toBeHidden();
  await menu(page, owned.reply, 'Unhide post'); await expect(hidden(page, owned.reply)).toBeVisible();
  await popup.evaluate(id => opener.postMessage(`done-report-${id}-demo`, location.origin), owned.reply);
  await expect(hidden(page, owned.reply)).toBeVisible(); await stranger.close(); await popup.close();
});

// Each case owns a separate thread: normal whole-thread deletion retires its
// report membership without clearing private history or bypassing admission.
for (const target of ['reply', 'id']) {
  test(`report hiding is idempotent for ${target}; reopening rejects a duplicate without side effects`, async ({ page, context, owned }) => {
    await page.goto('/demo/');
    const id = owned[target], popup = await openReport(page, id);
    const storageKey = `4chan-hide-${target === 'id' ? 't' : 'r'}-demo`;
    // Write without a storage event, leaving the controller's rendered state stale.
    await page.evaluate(({ storageKey, id }) => localStorage.setItem(storageKey,
      JSON.stringify({ [id]: Date.now() })), { storageKey, id });
    await popup.clock.install(); await popup.clock.pauseAt(new Date(Date.now() + 1000)); await submit(popup);
    await expect(hidden(page, id)).toBeHidden(); await popup.close();

    const storedHide = await page.evaluate(key => localStorage.getItem(key), storageKey);
    const cookies = await context.cookies();
    await page.evaluate(() => {
      window.duplicateReportMessages = [];
      window.addEventListener('message', event => {
        if (typeof event.data === 'string' && event.data.startsWith('done-report')) {
          window.duplicateReportMessages.push(event.data);
        }
      });
    });
    const response = context.waitForEvent('response', response =>
      response.url() === `${origin}/demo/imgboard.php?mode=report&no=${id}`
      && response.request().method() === 'GET');
    // The canonical GET rejects the duplicate before offering another form.
    const again = await openReport(page, id);
    const rejected = await response;
    expect(rejected.status()).toBe(422);
    expect(await rejected.headerValue('set-cookie')).toBe(null);
    await expect(again.locator('#report-popup-context')).toHaveAttribute('data-result', 'error');
    await expect(again.getByText('You have already reported this post.', { exact: true })).toBeVisible();
    await expect(again.locator('#report-form, #report-submit')).toHaveCount(0);
    await again.clock.install(); await again.clock.fastForward(5000);
    expect(again.isClosed()).toBe(false);
    await expect(hidden(page, id)).toBeHidden();
    expect(await page.evaluate(key => localStorage.getItem(key), storageKey)).toBe(storedHide);
    expect(await page.evaluate(() => window.duplicateReportMessages)).toEqual([]);
    expect(await context.cookies()).toEqual(cookies);
    await again.close();
  });
}

for (const mode of ['disabled', 'detached', 'replaced', 'teardown']) {
  test(`${mode} report target cannot acquire success authority`, async ({ page, owned }) => {
    await page.goto(owned.url);
    const popup = await openReport(page, owned.reply);
    await page.evaluate(({ mode, id }) => {
      if (mode === 'disabled') localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
      else if (mode === 'teardown') {
        window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
        window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
      } else {
        const post = document.getElementById(`p${id}`);
        if (mode === 'replaced') post.replaceWith(post.cloneNode(true)); else post.remove();
      }
    }, { mode, id: owned.reply });
    await popup.clock.install(); await popup.clock.pauseAt(new Date(Date.now() + 1000)); await submit(popup);
    expect(await page.evaluate(() => localStorage.getItem('4chan-hide-r-demo'))).toBe(null);
    await popup.close();
  });
}


// This catalog is synthetic test data, activated only inside its guarded scope.
// Every successful report owns a fresh thread, retired by ordinary deletion.
async function withCategoricalThread(request, callback) {
  return withReportCatalog(async catalog => {
    const password = 'owned-synthetic-categorical-report-password';
    const write = form => withPostingHistory(() => request.post('/demo/post', {
      headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password },
    }));
    const created = await write({ resto: '0', sub: catalog.marker, com: 'Synthetic categorical report OP' });
    expect(created.status()).toBe(303);
    const id = created.headers().location.match(/#p(\d+)$/)[1];
    try {
      const response = await write({ resto: id, com: 'Synthetic categorical report reply' });
      expect(response.status()).toBe(303);
      const reply = response.headers().location.match(/#p(\d+)$/)[1];
      await callback({ catalog, id, reply, url: `/demo/thread/${id}` });
    } finally {
      await withDeletionQuota(async () => {
        const removed = await request.post('/demo/delete', {
          headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
        });
        expect(removed.status()).toBe(303);
      });
    }
  });
}

const inspectCategorical = owned => owned.catalog.inspect({ op: owned.id, target: owned.reply });
function expectedCategory(catalog, kind) {
  return {
    revision: catalog.revision,
    id: kind === 'rule' ? catalog.ruleId : catalog.illegalId,
    kind: kind === 'rule' ? 1 : 2,
    baseWeight: kind === 'rule' ? 1.25 : 2.5,
    title: kind === 'rule' ? 'Synthetic board rule' : 'Synthetic illegal content',
  };
}

async function selectCategorical(popup, catalog, kind) {
  await expect(popup.locator('#reason')).toHaveCount(0);
  await expect(popup.locator('input[name="revision"]')).toHaveValue(String(catalog.revision));
  await expect(popup.locator('#report-category-select option')).toHaveText(['Synthetic board rule']);
  await popup.locator('#report-category-select').selectOption(String(catalog.ruleId));
  await popup.locator(`#report-category-${kind}`).check();
  if (kind === 'illegal') await expect(popup.locator('#report-category-select')).toBeDisabled();
  else await expect(popup.locator('#report-category-select')).toBeEnabled();
}

test('synthetic categorical radios switch the select and Close cancels without a report', async ({ page, request, context }) => {
  await withCategoricalThread(request, async owned => {
    await page.goto(owned.url);
    const cookies = await context.cookies();
    const popup = await openReport(page, owned.reply);
    await expect(popup.locator('#report-category-rule')).toBeChecked();
    await expect(popup.locator('#report-category-select')).toBeEnabled();
    await selectCategorical(popup, owned.catalog, 'illegal');
    await expect(popup.locator('#report-category-rule')).not.toBeChecked();
    await popup.locator('#report-category-rule').check();
    await expect(popup.locator('#report-category-illegal')).not.toBeChecked();
    await expect(popup.locator('#report-category-select')).toBeEnabled();
    await expect(popup.locator('#report-category-select')).toHaveValue(String(owned.catalog.ruleId));
    const closed = popup.waitForEvent('close');
    await popup.locator('#report-popup-close').click();
    await closed;
    await expect(hidden(page, owned.reply)).toBeVisible();
    expect(await inspectCategorical(owned)).toEqual({ reportCount: 0, categories: [] });
    expect(await context.cookies()).toEqual(cookies);
  });
});

for (const kind of ['rule', 'illegal']) {
  test(`synthetic ${kind} report commits captured metadata before the opener hides and closes at the boundary`, async ({ page, request }) => {
    await withCategoricalThread(request, async owned => {
      await page.goto(owned.url);
      const popup = await openReport(page, owned.reply);
      await popup.clock.install();
      await popup.clock.pauseAt(new Date(Date.now() + 1000));
      await selectCategorical(popup, owned.catalog, kind);
      const endpoint = `${origin}/demo/imgboard.php?mode=report&no=${owned.reply}`;
      let release, responseReady, responseFailed;
      const hold = new Promise(resolve => { release = resolve; });
      const fetched = new Promise((resolve, reject) => { responseReady = resolve; responseFailed = reject; });
      // Forward the real POST unchanged. Only hold its original response while
      // a separate database connection proves the committed metadata exists.
      await popup.route(endpoint, async route => {
        try {
          expect(route.request().method()).toBe('POST');
          const response = await route.fetch({ maxRedirects: 0 });
          responseReady(response);
          await hold;
          await route.fulfill({ response });
        } catch (error) {
          responseFailed(error);
          throw error;
        }
      }, { times: 1 });
      const submission = popup.locator('#report-submit').click();
      try {
        const response = await fetched;
        expect(response.status()).toBe(200);
        expect(await inspectCategorical(owned)).toEqual({
          reportCount: 1, categories: [expectedCategory(owned.catalog, kind)],
        });
        await expect(popup.locator('#report-popup-context')).toHaveAttribute('data-result', 'form');
        await expect(hidden(page, owned.reply)).toBeVisible();
        await expect(hidden(page, owned.id)).toBeVisible();
        expect(await page.evaluate(() => localStorage.getItem('4chan-hide-r-demo'))).toBe(null);
      } finally {
        release();
        await submission;
      }
      await expect(popup.locator('#report-popup-context')).toHaveAttribute('data-result', 'success');
      await expect(hidden(page, owned.reply)).toBeHidden();
      await expect(hidden(page, owned.id)).toBeVisible();
      await popup.clock.fastForward(2999);
      expect(popup.isClosed()).toBe(false);
      const closed = popup.waitForEvent('close');
      await popup.clock.fastForward(1);
      await closed;
      await expect(page).toHaveURL(`${origin}${owned.url}`);
    });
  });
}

test('synthetic categorical no-JavaScript form keeps its select enabled and illegal cat overrides cat_id', async ({ browser, request }) => {
  await withCategoricalThread(request, async owned => {
    const context = await browser.newContext({ javaScriptEnabled: false });
    try {
      const page = await context.newPage();
      const endpoint = `${origin}/demo/imgboard.php?mode=report&no=${owned.reply}`;
      await page.goto(endpoint);
      await expect(page.locator('#report-category-select')).toBeEnabled();
      await page.locator('#report-category-select').selectOption(String(owned.catalog.ruleId));
      await page.locator('#report-category-illegal').check();
      await expect(page.locator('#report-category-select')).toBeEnabled();
      await expect(page.locator('#report-popup-close')).toBeHidden();
      const sent = page.waitForRequest(r => r.url() === endpoint && r.method() === 'POST');
      await page.locator('#report-submit').click();
      const fields = new URLSearchParams((await sent).postData());
      expect(fields.get('cat')).toBe('31');
      expect(fields.get('cat_id')).toBe(String(owned.catalog.ruleId));
      expect(fields.get('revision')).toBe(String(owned.catalog.revision));
      await expect(page.locator('#report-popup-context')).toHaveAttribute('data-result', 'success');
      expect(await inspectCategorical(owned)).toEqual({
        reportCount: 1, categories: [expectedCategory(owned.catalog, 'illegal')],
      });
      await expect(page.locator('#report-popup-return')).toBeVisible();
      await expect(page.locator('#report-popup-return')).toHaveAttribute('href', '/demo/');
      await page.locator('#report-popup-return').click();
      await expect(page).toHaveURL(`${origin}/demo/`);
      await expect(hidden(page, owned.reply)).toBeVisible();
    } finally {
      await context.close();
    }
  });
});

for (const change of ['stale revision', 'disabled catalog']) {
  test(`synthetic categorical ${change} rejects without cookies, hiding, success messages or auto-close`, async ({ page, context, request }) => {
    await withCategoricalThread(request, async owned => {
      await page.goto(owned.url);
      const popup = await openReport(page, owned.reply);
      await popup.clock.install();
      await selectCategorical(popup, owned.catalog, 'rule');
      await page.evaluate(() => {
        window.categoricalReportMessages = [];
        window.addEventListener('message', event => {
          if (typeof event.data === 'string' && event.data.startsWith('done-report')) {
            window.categoricalReportMessages.push(event.data);
          }
        });
      });
      if (change === 'stale revision') {
        await popup.locator('input[name="revision"]').evaluate((input, revision) => {
          input.value = String(revision + 1);
        }, owned.catalog.revision);
      } else {
        await owned.catalog.disable();
      }
      const cookies = await context.cookies();
      const endpoint = `${origin}/demo/imgboard.php?mode=report&no=${owned.reply}`;
      const received = popup.waitForResponse(r => r.url() === endpoint && r.request().method() === 'POST');
      await popup.locator('#report-submit').click();
      const response = await received;
      expect(response.status()).toBe(422);
      expect(await response.headerValue('set-cookie')).toBe(null);
      await expect(popup.locator('#report-popup-context')).toHaveAttribute('data-result', 'error');
      await expect(popup.getByText(change === 'stale revision'
        ? 'Report categories changed. Please reload the report form.'
        : 'Categorical reporting is not active.', { exact: true })).toBeVisible();
      await expect(popup.getByRole('heading', { name: 'Report received', exact: true })).toHaveCount(0);
      await popup.clock.fastForward(5000);
      expect(popup.isClosed()).toBe(false);
      await expect(hidden(page, owned.reply)).toBeVisible();
      await expect(hidden(page, owned.id)).toBeVisible();
      expect(await page.evaluate(() => window.categoricalReportMessages)).toEqual([]);
      expect(await page.evaluate(() => localStorage.getItem('4chan-hide-r-demo'))).toBe(null);
      expect(await context.cookies()).toEqual(cookies);
      expect(await inspectCategorical(owned)).toEqual({ reportCount: 0, categories: [] });
      await expect(popup.locator('#report-popup-return')).toBeVisible();
      await popup.close();
    });
  });
}
