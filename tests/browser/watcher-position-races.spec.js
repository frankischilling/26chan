import { test, expect } from '@playwright/test';

test.use({ hasTouch: true });
const position = 'left: 20%; top: 10%;';
const initial = { threadWatcher: true, 'TW-position': position, unrelated: 'keep' };
const lock = 'paperboard-thread-watcher';
const stored = page => page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')));

test.beforeEach(async ({ context, page }) => {
  await context.addInitScript(initial => {
    if (!localStorage.getItem('4chan-settings')) localStorage.setItem('4chan-settings', JSON.stringify(initial));
  }, initial);
  await page.goto('/test/');
  await expect(page.locator('#threadWatcher')).toBeVisible();
});

async function point(page) {
  const box = await page.locator('.watcherTitle').boundingBox();
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}
async function drag(page, release = true) {
  const start = await point(page);
  const before = await page.locator('#threadWatcher').boundingBox();
  await page.mouse.move(start.x, start.y); await page.mouse.down();
  await page.mouse.move(start.x + 80, start.y + 40, { steps: 3 });
  await expect.poll(async () => (await page.locator('#threadWatcher').boundingBox()).x).toBeCloseTo(before.x + 80, 3);
  if (release) await page.mouse.up();
  return { start, before };
}
async function otherTab(context) {
  const other = await context.newPage(); await other.goto('/test/');
  await expect(other.locator('#threadWatcher')).toBeVisible();
  return other;
}
async function hold(other) {
  await other.evaluate(lock => {
    window.positionLockHeld = false;
    window.positionLockTask = navigator.locks.request(lock, async () => {
      window.positionLockHeld = true;
      await new Promise(resolve => { window.releasePositionLock = resolve; });
    });
  }, lock);
  await other.waitForFunction(() => window.positionLockHeld);
}
async function pending(page) {
  await page.waitForFunction(lock => navigator.locks.query().then(state => state.pending.some(entry => entry.name === lock)), lock);
}
async function release(other) {
  await other.evaluate(async lock => {
    window.releasePositionLock(); await window.positionLockTask;
    // A later same-name acquisition proves the earlier queued saves have settled.
    await navigator.locks.request(lock, () => true);
  }, lock);
}
async function update(other, value) {
  await other.evaluate(value => localStorage.setItem('4chan-settings', JSON.stringify(value)), value);
}

test('a trusted touch cancellation restores the starting position without saving', async ({ page, context }) => {
  const start = await point(page), before = await page.locator('#threadWatcher').boundingBox();
  await page.locator('#twHeader').evaluate(header => header.addEventListener('pointercancel', event => {
    window.positionCancelled = { trusted: event.isTrusted, type: event.pointerType };
  }));
  const cdp = await context.newCDPSession(page);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ ...start, id: 1 }] });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: start.x + 80, y: start.y + 40, id: 1 }] });
  await expect.poll(async () => (await page.locator('#threadWatcher').boundingBox()).x).toBeCloseTo(before.x + 80, 3);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchCancel', touchPoints: [] });
  await expect.poll(() => page.evaluate(() => window.positionCancelled)).toEqual({ trusted: true, type: 'touch' });
  await expect.poll(async () => (await page.locator('#threadWatcher').boundingBox()).x).toBeCloseTo(before.x, 3);
  expect(await stored(page)).toEqual(initial);
});

test('losing actual pointer capture cancels the drag and ignores its later release', async ({ page }) => {
  await page.locator('#twHeader').evaluate(header => {
    header.addEventListener('pointerdown', event => { window.positionPointer = event.pointerId; });
    header.addEventListener('lostpointercapture', event => { window.positionCaptureLost = event.isTrusted; });
  });
  const { start, before } = await drag(page, false);
  expect(await page.locator('#twHeader').evaluate(header => header.hasPointerCapture(window.positionPointer))).toBe(true);
  await page.locator('#twHeader').evaluate(header => header.releasePointerCapture(window.positionPointer));
  await page.mouse.move(start.x + 100, start.y + 50); await page.mouse.up();
  await expect.poll(() => page.evaluate(() => window.positionCaptureLost)).toBe(true);
  await expect.poll(async () => (await page.locator('#threadWatcher').boundingBox()).x).toBeCloseTo(before.x, 3);
  expect(await stored(page)).toEqual(initial);
});

test('changing fixed mode in another tab cancels an active drag', async ({ page, context }) => {
  const other = await otherTab(context);
  await expect(page.locator('#threadWatcher')).toHaveCSS('position', 'absolute');
  await drag(page, false);
  const newer = { ...initial, fixedThreadWatcher: true };
  await update(other, newer);
  await expect(page.locator('#threadWatcher')).toHaveCSS('position', 'fixed');
  await page.mouse.up();
  expect(await stored(page)).toEqual(newer);
  await expect(page.locator('#threadWatcher')).toHaveCSS('left', '256px');
});

test('a queued save succeeds and preserves unrelated settings changed by another tab', async ({ page, context }) => {
  const other = await otherTab(context);
  await hold(other); await drag(page); await pending(page);
  expect((await stored(page))['TW-position']).toBe(position);
  await update(other, { ...initial, unrelated: 'new tab value', preserved: true });
  await release(other);
  await expect.poll(async () => (await stored(page))['TW-position']).not.toBe(position);
  expect(await stored(page)).toMatchObject({ threadWatcher: true, unrelated: 'new tab value', preserved: true });
});

for (const [name, change] of [
  ['a newer position', { 'TW-position': 'right: 0; top: 5%;' }],
  ['a fixed-mode change', { fixedThreadWatcher: true }],
  ['global disabling', { disableAll: true }],
]) {
  test(`a queued save cannot overwrite ${name} from another tab`, async ({ page, context }) => {
    const other = await otherTab(context);
    await hold(other); await drag(page); await pending(page);
    const newer = { ...initial, ...change };
    await update(other, newer);
    await release(other);
    expect(await stored(page)).toEqual(newer);
    if (change.disableAll) await expect(page.locator('#threadWatcher')).toBeHidden();
    else if (change.fixedThreadWatcher) await expect(page.locator('#threadWatcher')).toHaveCSS('position', 'fixed');
    else await expect(page.locator('#threadWatcher')).toHaveCSS('right', '0px');
  });
}

test('navigation abandons a waiting position save without changing persisted coordinates', async ({ page, context }) => {
  const other = await otherTab(context);
  await hold(other); await drag(page); await pending(page);
  await page.goto('/test/catalog');
  await expect(page.locator('#threadWatcher')).toBeVisible();
  await release(other);
  expect(await stored(page)).toEqual(initial);
  await expect(page.locator('#threadWatcher')).toHaveCSS('left', '256px');
});
