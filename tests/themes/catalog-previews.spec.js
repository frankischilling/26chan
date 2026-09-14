import { test, expect } from '@playwright/test';

test.use({ javaScriptEnabled: true });
const now = new Date('2026-09-08T12:07:00Z');
async function freeze(page) {
  await page.clock.install({ time: now });
  await page.clock.pauseAt(now);
}
async function show(page, target) {
  await target.dispatchEvent('mouseover');
  await page.clock.runFor(250);
  await expect(page.locator('#post-preview')).toBeVisible();
}

for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  test(`catalog hover preview follows ${theme} source styling`, async ({ page, context }) => {
    await freeze(page);
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await page.goto('/demo/catalog');
    await page.locator('#teaser-ctrl').selectOption('off');
    const target = page.locator('#thread-1000001 .thumb'), tip = page.locator('#post-preview');
    await show(page, target);
    await expect(tip.locator('.post-subject')).toHaveText('What are you making?');
    await expect(tip.locator(':scope > .post-author')).toHaveText('Anonymous');
    await expect(tip.locator(':scope > .post-ago')).toHaveText('7 minutes ago');
    await expect(tip.locator('.post-page')).toHaveText('Page 1');
    await expect(tip.locator('.post-last')).toHaveText('Last reply by Synthetic reply author 2 minutes ago');
    await expect(tip.locator('.post-teaser')).toContainText('Share your latest paper project.');
    await expect(tip.locator('.post-teaser')).toContainText('Mine is another crane.');
    await expect(tip).toHaveCSS('font-size', '13.3333px');
    await expect(tip).toHaveCSS('padding', '5px 8px 4px');
    await expect(tip).toHaveCSS('background-color', theme === 'tomorrow' ? 'rgb(0, 0, 0)' : 'rgb(24, 31, 36)');
    await expect(tip.locator(':scope > .post-author')).toHaveCSS('color', theme === 'photon' ? 'rgb(222, 222, 222)' : 'rgb(0, 165, 80)');
    await expect(tip.locator('.post-subject')).toHaveCSS('color', theme === 'tomorrow' ? 'rgb(178, 148, 187)' : ['yotsuba-b', 'burichan', 'photon'].includes(theme) ? 'rgb(222, 222, 222)' : 'rgb(204, 17, 5)');
    await expect(tip).toHaveScreenshot(`catalog-preview-${theme}.png`);
    await target.dispatchEvent('mouseout');
    await expect(tip).toHaveCount(0);
    await page.locator('#teaser-ctrl').selectOption('on');
    await show(page, target);
    await expect(tip.locator('.post-teaser')).toHaveCount(0);
  });
}

test('hover delay, mouseout and catalog mutations cancel pending previews', async ({ page }) => {
  await freeze(page);
  await page.goto('/demo/catalog');
  const target = page.locator('#thread-1000001 .thumb'), tip = page.locator('#post-preview');
  await target.dispatchEvent('mouseover');
  await page.clock.runFor(249);
  await expect(tip).toHaveCount(0);
  await page.clock.runFor(1);
  await expect(tip).toBeVisible();
  await target.dispatchEvent('mouseout');
  await expect(tip).toHaveCount(0);
  await target.dispatchEvent('mouseover');
  await page.clock.runFor(249);
  await target.dispatchEvent('mouseout');
  await page.clock.runFor(1);
  await expect(tip).toHaveCount(0);
  await target.dispatchEvent('mouseover');
  await page.locator('#order-ctrl').selectOption('r');
  await page.clock.runFor(250);
  await expect(tip).toHaveCount(0);
  await show(page, target);
  await page.locator('#thread-1000001 .postMenuBtn').dispatchEvent('click');
  await expect(tip).toHaveCount(0);
  await page.keyboard.press('Escape');
  await target.dispatchEvent('mouseover');
  await page.locator('#qf-box').fill('^absent-owned-preview$');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  await page.clock.runFor(250);
  await expect(tip).toHaveCount(0);
  await expect(page.locator('#threads .thread')).toHaveCount(0);
});

test('text-date hover keeps the source date fallback and always includes its teaser', async ({ page }) => {
  await freeze(page);
  await page.goto('/text-catalog/catalog');
  const row = page.locator('#thread-1000401'), tip = page.locator('#post-preview');
  await show(page, row.locator('.txt-date'));
  await expect(tip.locator('.post-subject')).toHaveCount(0);
  await expect(tip).toContainText('Posted by Anonymous one day ago');
  await expect(tip.locator('.post-teaser')).toHaveText('Synthetic counter fixture.');
  await expect(tip.locator('.post-last .post-ago')).toHaveText('2 minutes ago');
  await expect(tip).toHaveScreenshot('catalog-preview-text.png');
  await row.locator('.txt-date').dispatchEvent('mouseout');
  await row.locator('.txt-sub a').dispatchEvent('mouseover');
  await page.clock.runFor(250);
  await expect(tip).toHaveCount(0);
  await page.setViewportSize({ width: 480, height: 900 });
  await expect(row.locator('.txt-date')).toBeHidden();
});

test('relative durations retain source singular and remainder boundaries', async ({ page }) => {
  await freeze(page);
  await page.goto('/demo/catalog');
  const target = page.locator('#thread-1000001 .thumb');
  for (const [seconds, label] of [[0, 'less than a second'], [1, 'less than a second'], [2, '2 seconds'], [59, '59 seconds'], [60, 'one minute'], [119, 'one minute'], [120, '2 minutes'], [3600, 'one hour'], [3660, 'one hour'], [3720, 'one hour and 2 minutes'], [7200, '2 hours'], [86400, 'one day'], [90000, 'one day'], [93600, 'one day and 2 hours'], [172800, '2 days']]) {
    await target.dispatchEvent('mouseout');
    await page.evaluate(seconds => {
      const root = document.querySelector('template.catalogPreview').content.firstElementChild;
      root.dataset.createdAt = String(Math.floor((Date.now() + 250) / 1000) - seconds);
    }, seconds);
    await show(page, target);
    await expect(page.locator('#post-preview > .post-ago')).toHaveText(`${label} ago`);
  }
});

test('preview position follows source right/left and viewport-bottom rules with scroll offsets', async ({ page }) => {
  await freeze(page);
  await page.goto('/demo/catalog');
  await page.locator('#teaser-ctrl').selectOption('off');
  const target = page.locator('#thread-1000001 .thumb');
  for (const [left, top, scroll] of [[20, 40, 0], [1000, 40, 0], [1000, 850, 0], [20, 40, 500]]) {
    await target.dispatchEvent('mouseout');
    await page.evaluate(scroll => {
      document.body.style.minHeight = '1800px';
      window.scrollTo(0, scroll);
    }, scroll);
    expect(await page.evaluate(() => window.scrollY)).toBe(scroll);
    await target.evaluate((node, { left, top }) => {
      Object.assign(node.style, { position: 'fixed', left: `${left}px`, top: `${top}px` });
    }, { left, top });
    await show(page, target);
    const position = await target.evaluate(node => {
      const rect = node.getBoundingClientRect(), tip = document.getElementById('post-preview');
      const width = document.documentElement.offsetWidth, height = document.documentElement.clientHeight;
      let top = rect.top + tip.offsetHeight > height ? height - tip.offsetHeight - 20 : rect.top;
      return { actualLeft: parseFloat(tip.style.left), actualTop: parseFloat(tip.style.top),
        left: (width - rect.right < (width * .3 | 0) ? rect.left - tip.offsetWidth - 5 : rect.right + 5) + scrollX,
        top: (top < 0 ? 3 : top) + scrollY };
    });
    expect(position.actualLeft).toBeCloseTo(position.left, 2);
    expect(position.actualTop).toBeCloseTo(position.top, 2);
  }
});

test('preview page numbers retain bump ranking after sort, pin and filtering', async ({ page }) => {
  await freeze(page);
  await page.goto('/preview-pages/catalog');
  const target = page.locator('#thread-1000401 .thumb'), label = page.locator('#post-preview .post-page');
  await show(page, target);
  const original = await label.textContent();
  expect(original).toBe('Page 3');
  await page.locator('#order-ctrl').selectOption('r');
  await show(page, target);
  await expect(label).toHaveText(original);
  await page.locator('#thread-1000401 .postMenuBtn').dispatchEvent('click');
  await page.getByRole('menuitem', { name: 'Pin thread', exact: true }).dispatchEvent('click');
  await expect(page.locator('#threads > .thread').nth(1)).toHaveAttribute('id', 'thread-1000401');
  await expect(target).toHaveClass(/\bpinned\b/);
  await show(page, target);
  await expect(label).toHaveText(original);
  await page.locator('#qf-box').fill('Both limits reached');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  await show(page, target);
  await expect(label).toHaveText(original);
});
