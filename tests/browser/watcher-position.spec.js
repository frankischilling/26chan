import { test, expect } from '@playwright/test';
import { saveWatcherSettings } from './helpers/watcher-settings.js';

async function seed(context, position = 'left: 20%; top: 10%; position: fixed;') {
  await context.addInitScript(position => {
    if (!localStorage.getItem('4chan-settings')) localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true, unrelated: 'keep', 'TW-position': position }));
  }, position);
}
async function drag(page, x, y, release = true) {
  const title = await page.locator('.watcherTitle').boundingBox();
  await page.mouse.move(title.x + title.width / 2, title.y + title.height / 2);
  await page.mouse.down();
  await page.mouse.move(title.x + title.width / 2 + x, title.y + title.height / 2 + y, { steps: 5 });
  if (release) await page.mouse.up();
}
const stored = page => page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings'))['TW-position']);

test('dragged positions persist across reloads and tabs without changing unrelated preferences', async ({ page, context }) => {
  await seed(context);
  await page.goto('/test/catalog');
  const panel = page.locator('#threadWatcher');
  await expect(panel).toHaveCSS('position', 'absolute');
  const before = await panel.boundingBox();
  await drag(page, 160, 90);
  await expect.poll(() => stored(page)).not.toContain('position: fixed');
  const after = await panel.boundingBox();
  expect(after.x).toBeCloseTo(before.x + 160, 3);
  expect(after.y).toBeCloseTo(before.y + 90, 3);
  await page.reload();
  expect((await panel.boundingBox()).x).toBeCloseTo(after.x, 3);
  expect((await panel.boundingBox()).y).toBeCloseTo(after.y, 3);
  const other = await context.newPage();
  await other.goto('/test/catalog');
  await drag(page, 50, 30);
  await expect.poll(async () => (await other.locator('#threadWatcher').boundingBox()).x).toBeCloseTo(after.x + 50, 3);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).unrelated)).toBe('keep');
  await page.locator('#twHeader').focus();
  await page.locator('#twHeader').press('Shift+ArrowRight');
  await expect.poll(async () => (await other.locator('#threadWatcher').boundingBox()).x).toBeCloseTo(after.x + 60, 3);
});

test('a cross-tab position change cancels an active drag instead of being overwritten on release', async ({ page, context }) => {
  await seed(context);
  await page.goto('/test/catalog');
  const other = await context.newPage();
  await other.goto('/test/catalog');
  await drag(page, 100, 50, false);
  await other.evaluate(() => {
    const settings = JSON.parse(localStorage.getItem('4chan-settings'));
    localStorage.setItem('4chan-settings', JSON.stringify({ ...settings, 'TW-position': 'left: 64px; top: 45px;' }));
  });
  await expect(page.locator('#threadWatcher')).toHaveCSS('left', '64px');
  await page.mouse.up();
  expect(await stored(page)).toBe('left: 64px; top: 45px;');
  await expect(page.locator('#threadWatcher')).toHaveCSS('top', '45px');
});

test('invalid stored styles fall back safely and mobile placement leaves desktop coordinates intact', async ({ page, context }) => {
  await seed(context);
  await page.goto('/test/catalog');
  for (const raw of ['left: 0; top: 0; background: url(/position-denied.svg);', 'left: -10px; top: 0;', { left: '0px' }]) {
    await page.evaluate(raw => localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true, 'TW-position': raw })), raw);
    await page.reload();
    await expect(page.locator('#threadWatcher')).toHaveCSS('left', '10px');
    await expect(page.locator('#threadWatcher')).toHaveCSS('top', '75px');
    expect(await page.locator('#threadWatcher').evaluate(node => node.style.background)).toBe('');
  }
  await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true, 'TW-position': 'right: 0; bottom: 0;' })));
  await page.reload();
  await expect(page.locator('#threadWatcher')).toHaveCSS('right', '0px');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator('#watcher-open-mobile').click();
  await expect(page.locator('#threadWatcher')).toHaveCSS('left', '0px');
  expect(await stored(page)).toBe('right: 0; bottom: 0;');
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(page.locator('#threadWatcher')).toHaveCSS('right', '0px');
  await expect(page.locator('#threadWatcher')).toHaveCSS('bottom', '0px');
});

test('dragging still works in the current tab when storage is unavailable', async ({ page, context }) => {
  await context.addInitScript(() => { for (const method of ['getItem', 'setItem', 'removeItem']) Storage.prototype[method] = () => { throw new Error('Unavailable'); }; });
  await page.goto('/test/catalog');
  await saveWatcherSettings(page, { threadWatcher: true }, { reload: false });
  const before = await page.locator('#threadWatcher').boundingBox();
  await drag(page, 200, 100);
  await expect.poll(async () => (await page.locator('#threadWatcher').boundingBox()).x).toBeCloseTo(before.x + 200, 3);
  await expect(page.locator('.watcherNotice')).toContainText('Changes stay in this tab');
});
