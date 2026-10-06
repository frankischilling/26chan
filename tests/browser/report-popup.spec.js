import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
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
  closed = popup.waitForEvent('close'); await popup.keyboard.press('Escape'); await closed;
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

test('report hiding is idempotent even when a newer stored hide has not reached this tab', async ({ page, owned }) => {
  for (const target of ['reply', 'id']) {
    await page.goto('/demo/');
    const id = owned[target], popup = await openReport(page, id);
    // Write without a storage event, leaving the controller's rendered state stale.
    await page.evaluate(({ target, id }) => localStorage.setItem(`4chan-hide-${target === 'id' ? 't' : 'r'}-demo`,
      JSON.stringify({ [id]: Date.now() })), { target, id });
    await popup.clock.install(); await popup.clock.pauseAt(new Date(Date.now() + 1000)); await submit(popup);
    await expect(hidden(page, id)).toBeHidden(); await popup.close();
    const again = await openReport(page, id);
    await again.clock.install(); await again.clock.pauseAt(new Date(Date.now() + 1000)); await submit(again);
    await expect(hidden(page, id)).toBeHidden(); await again.close();
  }
});

test('disabled hiding, detached targets, teardown and replaced posts cannot acquire success authority', async ({ page, owned }) => {
  for (const mode of ['disabled', 'detached', 'replaced', 'teardown']) {
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
    await popup.close(); await page.evaluate(() => localStorage.removeItem('4chan-settings'));
  }
});
