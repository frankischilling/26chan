import { test as base, expect } from '@playwright/test';

const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-post-menu-event-password';
    const write = form => request.post('/demo/post', {
      headers: { Origin: 'http://127.0.0.1:3000' }, maxRedirects: 0, form: { ...form, password },
    });
    const response = await write({ resto: '0', sub: 'Owned menu event', com: 'Owned OP for menu events' });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/thread\/(\d+)/)[1];
    try {
      const response = await write({ resto: id, com: 'Owned reply for menu events' });
      expect(response.status()).toBe(303);
      await use({ id, reply: response.headers().location.match(/#p(\d+)/)[1], url: `/demo/thread/${id}` });
    } finally {
      const response = await request.post('/demo/delete', {
        headers: { Origin: 'http://127.0.0.1:3000' }, maxRedirects: 0, form: { no: id, password },
      });
      expect(response.status()).toBe(303);
    }
  },
});
const trigger = (page, id) => page.getByRole('button', { name: `Post menu for post ${id}`, exact: true });

async function recordEvents(page) {
  await page.addInitScript(() => {
    window.menuReadyEvents = [];
    document.addEventListener('4chanPostMenuReady', event => {
      const { postId, isOP, node } = event.detail;
      const trigger = document.querySelector(`[data-post-menu][aria-label="Post menu for post ${postId}"]`);
      window.menuReadyEvents.push({ postId, isOP, keys: Object.keys(event.detail).sort(),
        constructor: event.constructor.name, bubbles: event.bubbles, cancelable: event.cancelable,
        target: event.target === document, connected: node.isConnected, tag: node.tagName,
        parent: node.parentElement.id, active: trigger?.classList.contains('menuOpen'),
        expanded: trigger?.getAttribute('aria-expanded'), labels: [...node.querySelectorAll('[role="menuitem"]')].map(item => item.textContent) });
      window.lastMenuReadyNode = node;
    });
  });
}

test('native menu-ready events expose exact OP/reply identity and complete detached menus', async ({ page, owned }) => {
  await recordEvents(page);
  await page.goto('/demo/');
  await trigger(page, owned.id).click();
  const expected = { postId: owned.id, isOP: true, keys: ['isOP', 'node', 'postId'],
    constructor: 'Event', bubbles: false, cancelable: false, target: true, connected: false,
    tag: 'UL', parent: 'post-menu', active: true, expanded: 'true', labels: ['Report post', 'Hide thread'] };
  expect(await page.evaluate(() => window.menuReadyEvents)).toEqual([expected]);
  expect(await page.evaluate(() => window.lastMenuReadyNode === document.querySelector('#post-menu > ul'))).toBe(true);
  await page.keyboard.press('Escape');
  await trigger(page, owned.reply).click();
  expect(await page.evaluate(() => window.menuReadyEvents.at(-1))).toEqual({
    ...expected, postId: owned.reply, isOP: false, labels: ['Report post', 'Hide post'],
  });
  await page.goto(owned.url);
  await trigger(page, owned.id).click();
  expect(await page.evaluate(() => window.menuReadyEvents)).toEqual([{ ...expected, labels: ['Report post'] }]);
});

test('same-document subscribers can augment the live list before placement and keyboard focus', async ({ page, owned }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    window.addonClicks = 0;
    document.addEventListener('4chanPostMenuReady', event => {
      window.addonConnected = event.detail.node.isConnected;
      const row = document.createElement('li'); row.setAttribute('role', 'none');
      const control = document.createElement('button'); control.type = 'button';
      control.setAttribute('role', 'menuitem'); control.textContent = 'Owned menu subscriber';
      control.addEventListener('click', () => { window.addonClicks++; });
      row.append(control); event.detail.node.append(row);
    });
  });
  await page.goto(owned.url);
  await trigger(page, owned.reply).press('ArrowUp');
  const added = page.getByRole('menuitem', { name: 'Owned menu subscriber', exact: true });
  await expect(added).toBeFocused();
  expect(await page.evaluate(() => window.addonConnected)).toBe(false);
  const box = await page.locator('#post-menu').boundingBox();
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(390);
  await added.press('Enter');
  expect(await page.evaluate(() => window.addonClicks)).toBe(1);
  await page.keyboard.press('Escape');
  await expect(trigger(page, owned.reply)).toBeFocused();
});

test('closing, storage synchronization and global disabling do not republish menu-ready events', async ({ page, context, owned }) => {
  await recordEvents(page);
  await page.goto('/demo/');
  await trigger(page, owned.id).click();
  await trigger(page, owned.id).click();
  await expect(page.locator('#post-menu')).toHaveCount(0);
  expect(await page.evaluate(() => window.menuReadyEvents.length)).toBe(1);
  await trigger(page, owned.id).click();
  const other = await context.newPage(); await other.goto(owned.url);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true })));
  await expect(page.getByRole('menuitem', { name: 'Add to watch list', exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.menuReadyEvents.length)).toBe(2);
  await trigger(page, owned.id).evaluate(element => { window.savedMenuTrigger = element; });
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await expect(page.locator('#post-menu')).toHaveCount(0);
  await page.evaluate(() => window.savedMenuTrigger.click());
  expect(await page.evaluate(() => window.menuReadyEvents.length)).toBe(2);
});
