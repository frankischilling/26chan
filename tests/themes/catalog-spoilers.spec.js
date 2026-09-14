import { test, expect } from '@playwright/test';

test.use({ javaScriptEnabled: true });

// Keep bounded diagnostics in CI logs even when no trace artifact is available.
const testWithDiagnostics = test.extend({
  catalogDiagnostics: [async ({ page }, use, info) => {
    const errors = [], scripts = [];
    page.on('pageerror', error => { if (errors.length < 8) errors.push(error.message.slice(0, 1024)); });
    page.on('response', response => {
      if (response.url().endsWith('/static/catalog-preferences.v1.js') && scripts.length < 8) scripts.push(response.status());
    });
    await use();
    if (info.status !== info.expectedStatus) {
      const state = await page.evaluate(() => ({
        ready: document.readyState,
        spoilers: document.querySelector('#theme-nospoiler')?.value,
        query: new URL(location.href).searchParams.get('spoilers'),
        reveal: document.body.classList.contains('reveal-img-spoilers'),
        menus: document.querySelectorAll('#threads .postMenuBtn').length,
        images: Array.from(document.querySelectorAll('#threads img[data-spoiler-src]')).slice(0, 4)
          .map(node => ({ id: node.id, src: node.getAttribute('src'), width: node.width, height: node.height })),
      })).catch(() => ({ unavailable: true }));
      console.log('Catalog spoiler failure diagnostics:', JSON.stringify({ errors, scripts, state }));
    }
  }, { auto: true }],
});

for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
  testWithDiagnostics(`spoiler reveal keeps links, dimensions and deletion states in ${theme}`, async ({ page, context }) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    for (const [width, height] of [[1280, 900], [390, 844]]) {
      await page.setViewportSize({ width, height });
      await page.goto('/img/catalog?size=small&spoilers=off&q=');
      const image = page.locator('#threads img[data-spoiler-src]').first();
      const original = await image.evaluate(node => ({ source: node.dataset.spoilerSrc, href: node.closest('a').getAttribute('href'), small: [Number(node.dataset.smallWidth), Number(node.dataset.smallHeight)], large: [Number(node.dataset.largeWidth), Number(node.dataset.largeHeight)] }));
      await expect(image).toHaveAttribute('src', '/static/catalog/spoiler.png');
      await page.getByLabel('Spoilers:', { exact: true }).selectOption('on');
      await expect(image).toHaveAttribute('src', original.source);
      await expect(image).not.toHaveClass(/spoilerImage/);
      expect(await image.evaluate(node => [node.width, node.height])).toEqual(original.small);
      await page.locator('#size-ctrl').selectOption('large');
      expect(await image.evaluate(node => [node.width, node.height])).toEqual(original.large);
      expect(await image.locator('..').getAttribute('href')).toBe(original.href);
      await expect(page.locator('.fileDeleted')).toHaveAttribute('src', '/static/catalog/filedeleted-res.gif');
      await page.getByLabel('Spoilers:', { exact: true }).selectOption('off');
      await expect(image).toHaveAttribute('src', '/static/catalog/spoiler.png');
      expect(await image.evaluate(node => [node.width, node.height])).toEqual([100, 100]);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    }
  });
}

test('stored reveal does not fetch hidden or filtered spoiler thumbnails until shown', async ({ page }) => {
  await page.goto('/img/catalog?spoilers=off&q=');
  const data = await page.locator('#threads img[data-spoiler-src]').first().evaluate(node => ({ id: node.closest('.thread').dataset.threadId, source: node.dataset.spoilerSrc }));
  await page.evaluate(({ id }) => {
    localStorage.setItem('catalog-theme', JSON.stringify({ nospoiler: true, css: 'body{display:none}' }));
    localStorage.setItem('4chan-hide-t-img', JSON.stringify({ [id]: true }));
  }, data);
  const requested = [];
  page.on('request', request => requested.push(request.url()));
  await page.goto('/img/catalog?q=');
  await expect(page.locator(`#thread-${data.id}`)).toHaveCount(0);
  expect(requested).not.toContain(data.source);
  await expect(page.getByLabel('Spoilers:', { exact: true })).toHaveValue('on');
  await page.locator('#filters-clear-hidden').click();
  const image = page.locator(`#thread-${data.id} img[data-spoiler-src]`);
  await image.scrollIntoViewIfNeeded();
  await expect(image).toHaveAttribute('src', data.source);
  await expect.poll(() => requested.includes(data.source)).toBe(true);
  await expect.poll(() => image.evaluate(node => node.complete && node.naturalWidth > 0)).toBe(true);
  await page.evaluate(() => localStorage.removeItem('4chan-hide-t-img'));
  requested.length = 0;
  await page.goto('/img/catalog?q=^NO_MATCH_EXPECTED$');
  await expect(page.locator('#threads > .thread')).toHaveCount(0);
  expect(requested).not.toContain(data.source);
  await page.locator('#qf-box').fill('');
  await expect(page.locator(`#thread-${data.id}`)).toBeVisible();
  await page.locator(`#thread-${data.id} img`).scrollIntoViewIfNeeded();
  await expect.poll(() => requested.includes(data.source)).toBe(true);
});

test('finite reveal storage, explicit URLs, reset and unavailable storage stay safe', async ({ page }) => {
  await page.goto('/img/catalog?spoilers=off&q=');
  for (const stored of ['null', '[]', '{', '{"nospoiler":"true"}', '{"nospoiler":1}', 'x'.repeat(4097)]) {
    await page.evaluate(raw => localStorage.setItem('catalog-theme', raw), stored);
    await page.goto('/img/catalog?q=');
    await expect(page.getByLabel('Spoilers:', { exact: true })).toHaveValue('off');
    await expect(page.locator('#threads img[data-spoiler-src]').first()).toHaveAttribute('src', '/static/catalog/spoiler.png');
  }
  await page.evaluate(() => localStorage.setItem('catalog-theme', '{"nospoiler":true}'));
  await page.goto('/img/catalog?spoilers=off&q=');
  await expect(page.getByLabel('Spoilers:', { exact: true })).toHaveValue('off');
  await page.goto('/img/catalog?q=');
  await expect(page.getByLabel('Spoilers:', { exact: true })).toHaveValue('on');
  await page.locator('#catalog-reset').click();
  await expect(page.getByLabel('Spoilers:', { exact: true })).toHaveValue('off');
  expect(await page.evaluate(() => localStorage.getItem('catalog-theme'))).toBeNull();
  await page.addInitScript(() => { for (const name of ['getItem', 'setItem', 'removeItem']) Storage.prototype[name] = () => { throw new Error('unavailable'); }; });
  await page.goto('/img/catalog?spoilers=off&q=');
  await page.getByLabel('Spoilers:', { exact: true }).selectOption('on');
  await expect(page.locator('#threads img[data-spoiler-src]').first()).not.toHaveAttribute('src', '/static/catalog/spoiler.png');
});
