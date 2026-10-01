import { test as base, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-tooltip-password';
    const name = '<img src="/owned-tooltip-attack"> & Owned full name';
    const response = await request.post('/demo/post', { headers: { Origin: origin, Connection: 'close' },
      maxRedirects: 0, form: { resto: '0', name, password, com: 'Owned tooltip OP', sub: 'Owned tooltip subject' } });
    expect(response.status()).toBe(303);
    const no = response.headers().location.match(/#p(\d+)$/)[1];
    try { await use({ no, name, url: `/demo/thread/${no}` }); }
    finally {
      const deleted = await request.post('/demo/delete', { headers: { Origin: origin, Connection: 'close' },
        maxRedirects: 0, form: { no, password } });
      expect(deleted.status()).toBe(303);
    }
  },
});

test('persisted mobile full-name and relative-date tooltips contain inert text and retain number links', async ({ page, owned }) => {
  const requests = [], errors = [];
  page.on('request', request => { if (request.url().includes('/owned-tooltip-attack')) requests.push(request.url()); });
  page.on('pageerror', error => errors.push(error.message));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(owned.url);
  await page.evaluate(() => {
    window.ownedCallbackCalls = 0;
    window.mShowFull = () => { window.ownedCallbackCalls++; throw new Error('untrusted callback'); };
  });
  const name = page.locator(`#pim${owned.no} .name`), tip = page.locator('#tooltip');
  await name.hover(); await expect(tip).toHaveText(owned.name);
  await expect(tip).toHaveAttribute('role', 'tooltip'); await expect(tip.locator('*')).toHaveCount(0);
  await expect(name).toHaveAttribute('aria-describedby', 'tooltip');
  await expect(name).toHaveAttribute('title', owned.name);
  await page.mouse.move(0, 0); await expect(tip).toHaveCount(0);
  await expect(name).not.toHaveAttribute('aria-describedby');
  const date = page.locator(`#pim${owned.no} .dateTime`);
  const title = await date.getAttribute('title');
  await page.clock.setFixedTime(new Date((Number(await date.getAttribute('data-utc')) + 3720) * 1000));
  await date.locator('a').first().hover(); await expect(tip).toHaveText('one hour and 2 minutes ago');
  await expect(date).not.toHaveAttribute('title');
  expect(await date.locator('a').allTextContents()).toEqual(['No.', owned.no]);
  await page.mouse.move(0, 0); await expect(tip).toHaveCount(0);
  await expect(date).toHaveAttribute('title', title);
  expect(await page.evaluate(() => window.ownedCallbackCalls)).toBe(0);
  await page.locator('.thread-stats .ts-replies').first().hover();
  await expect(tip).toHaveText('Replies');
  await page.mouse.move(0, 0); await expect(tip).toHaveCount(0);
  expect(requests).toEqual([]); expect(errors).toEqual([]);
});

test('visible tooltips cancel on viewport, disablement, BFCache and detached original posts', async ({ page, context, owned }) => {
  await page.setViewportSize({ width: 390, height: 844 }); await page.goto(owned.url);
  const name = page.locator(`#pim${owned.no} .name`), tip = page.locator('#tooltip');
  await name.hover(); await expect(tip).toBeVisible();
  await page.setViewportSize({ width: 1280, height: 900 }); await expect(tip).toHaveCount(0);
  await expect(name).not.toHaveAttribute('aria-describedby');
  await page.locator(`#pi${owned.no} .name`).hover(); await expect(tip).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 }); await name.hover(); await expect(tip).toBeVisible();
  const other = await context.newPage();
  try {
    await other.goto(owned.url);
    await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
    await expect(tip).toHaveCount(0); await expect(name).toHaveAttribute('title', owned.name);
    await other.evaluate(() => localStorage.removeItem('4chan-settings'));
    await page.mouse.move(0, 0); await name.hover(); await expect(tip).toBeVisible();
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
    await expect(tip).toHaveCount(0);
    await page.evaluate(() => window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
    await page.mouse.move(0, 0); await name.hover(); await expect(tip).toBeVisible();
    await page.evaluate(no => document.getElementById(`pc${no}`).remove(), owned.no);
    await expect(tip).toHaveCount(0);
  } finally { await other.close(); }
});

test('forged headers and callback attributes cannot create tooltips or gain HTML or script authority', async ({ page, owned }) => {
  await page.setViewportSize({ width: 390, height: 844 }); await page.goto(owned.url);
  const tip = page.locator('#tooltip');
  await page.evaluate(no => {
    const label = document.querySelector(`#pim${no} .name`);
    label.title = 'Forged full name'; label.setAttribute('data-tip-cb', 'ownedUnexpectedCallback');
    window.ownedCallbackCalls = 0; window.ownedUnexpectedCallback = () => { window.ownedCallbackCalls++; };
  }, owned.no);
  await page.locator(`#pim${owned.no} .name`).hover();
  // Advance only virtual timers, preserving the real hover and production DOM.
  await page.clock.install(); await page.clock.fastForward(1000);
  await expect(tip).toHaveCount(0);
  await page.evaluate(no => {
    document.querySelector(`#pim${no} .name`).title = document.querySelector(`#pi${no} .name`).textContent;
    document.querySelector(`#pim${no} .dateTime`).dataset.utc = '999999999999999999999999';
  }, owned.no);
  await page.locator(`#pim${owned.no} .dateTime a`).first().hover();
  await page.clock.fastForward(1000); await expect(tip).toHaveCount(0);
  expect(await page.evaluate(() => window.ownedCallbackCalls)).toBe(0);
});

test('hover delays and pending cancellation retain the released name and date timing', async ({ page, owned }) => {
  await page.setViewportSize({ width: 390, height: 844 }); await page.goto(owned.url);
  await page.clock.install(); await page.clock.pauseAt(new Date(Date.now() + 1000));
  const name = page.locator(`#pim${owned.no} .name`), tip = page.locator('#tooltip');
  await name.dispatchEvent('mouseover');
  await page.clock.fastForward(299); await expect(tip).toHaveCount(0);
  await page.clock.fastForward(1); await expect(tip).toHaveText(owned.name);
  await name.dispatchEvent('mouseout'); await expect(tip).toHaveCount(0);
  const link = page.locator(`#pim${owned.no} .dateTime a`).first();
  await link.dispatchEvent('mouseover');
  await page.clock.fastForward(499); await expect(tip).toHaveCount(0);
  await page.clock.fastForward(1); await expect(tip).toHaveCount(1);
  await link.dispatchEvent('mouseout'); await expect(tip).toHaveCount(0);
  await name.dispatchEvent('mouseover');
  await page.clock.fastForward(299); await name.dispatchEvent('mouseout');
  await page.clock.fastForward(1000); await expect(tip).toHaveCount(0);
  await expect(name).not.toHaveAttribute('aria-describedby');
  await expect(name).toHaveAttribute('title', owned.name);
});
