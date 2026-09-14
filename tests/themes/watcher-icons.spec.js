import { test, expect } from '@playwright/test';

const themes = { yotsuba: 'futaba', 'yotsuba-b': 'burichan', futaba: 'futaba',
  burichan: 'burichan', tomorrow: 'tomorrow', photon: 'photon' };

async function expectIcon(control, family, name, scale, catalog) {
  const filename = `${name}${scale === 2 ? '@2x' : ''}.${name === 'post_expand_rotate' ? 'gif' : 'png'}`;
  const path = `/static/watcher/${family}/${filename}`;
  await expect(control).toHaveCSS('width', '18px');
  await expect(control).toHaveCSS('height', '18px');
  if (catalog) {
    await expect(control).toHaveCSS('background-image', `url("http://127.0.0.1:3000${path}")`);
    await expect(control.locator('img')).toHaveCount(0);
  } else {
    await expect(control.locator('img')).toHaveAttribute('src', path);
    await expect(control.locator('img')).toHaveAttribute('alt', '');
  }
  const dimensions = await control.evaluate(async element => {
    const source = element.querySelector('img')?.src
      || getComputedStyle(element).backgroundImage.slice(5, -2);
    const image = new Image();
    const loaded = new Promise((resolve, reject) => {
      image.onload = () => resolve([image.naturalWidth, image.naturalHeight]);
      image.onerror = () => reject(new Error(`Icon failed to load: ${source}`));
    });
    image.src = source;
    return loaded;
  });
  expect(dimensions).toEqual([18 * scale, 18 * scale]);
}

for (const scale of [1, 2]) {
  test.describe(`watcher icons at ${scale}x`, () => {
    test.use({ javaScriptEnabled: true, deviceScaleFactor: scale });
    for (const [theme, family] of Object.entries(themes)) {
      test(`${theme} uses pinned icons across catalog/thread, mobile, watch and refresh states`, async ({ page, context }) => {
        await context.addCookies([{ name: 'board-theme-ws', value: theme,
          url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
        await context.addInitScript(() => {
          const threadFixture = location.pathname.startsWith('/img/thread/');
          const id = threadFixture ? 1000201 : 1000001;
          const board = threadFixture ? 'img' : 'demo';
          localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
          localStorage.setItem('4chan-watch', JSON.stringify({
            [`${id}-${board}`]: ['Wide watcher label '.repeat(3).slice(0, 45), id, 0, false, false],
          }));
          localStorage.setItem('4chan-tw-timestamp', String(Date.now()));
        });
        const pending = [];
        await page.route('**/_watch/**', route => { pending.push(route); });
        for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
          await page.setViewportSize(viewport);
          for (const catalog of [true, false]) {
            await page.goto(catalog ? '/demo/catalog' : '/img/thread/1000201');
            if (viewport.width === 390) await page.locator('#watcher-open-mobile').click();
            const panel = page.locator('#threadWatcher');
            const refresh = page.getByRole('button', { name: 'Refresh', exact: true });
            await expect(panel).toBeVisible();
            await expect(page.locator('#twHeader')).toHaveCSS('height', '17px');
            await expect(panel).toHaveCSS('padding-top', '3px');
            await expect(panel).toHaveCSS('max-width', viewport.width === 390 ? '100%' : '265px');
            await expect(page.locator('#watchList li').first()).toHaveCSS('white-space', 'nowrap');
            await expect(page.locator('#watchList li').first()).toHaveCSS('text-overflow', 'ellipsis');
            await expectIcon(refresh, family, 'refresh', scale, catalog);
            if (viewport.width === 390) {
              await expectIcon(page.locator('#twClose'), family, 'cross', scale, catalog);
              await page.getByRole('button', { name: 'Close', exact: true }).click();
              await expect(panel).toBeHidden();
              await page.locator('#watcher-open-mobile').click();
              await expect(panel).toBeVisible();
            }
            const control = catalog ? page.locator('#leaf-1000001') : page.locator('.threadNav:visible .wbtn').first();
            await expect(control).toHaveAttribute('aria-pressed', 'true');
            await expectIcon(control, family, 'watch_thread_on', scale, catalog);
            if (catalog) {
              expect(await control.evaluate(element => element.parentElement.firstElementChild === element)).toBe(true);
              await expect(page.locator('.meta .wbtn')).toHaveCount(0);
              await page.mouse.move(1, 1);
              await expect(control).toHaveCSS('opacity', '0');
            }
            await control.focus();
            await expect(control).toBeFocused();
            await expect(control).toHaveCSS('opacity', '1');
            await control.press('Space');
            await expect(control).toHaveAttribute('aria-pressed', 'false');
            await expectIcon(control, family, 'watch_thread_off', scale, catalog);
            await control.press('Enter');
            await expect(control).toHaveAttribute('aria-pressed', 'true');
            await expectIcon(control, family, 'watch_thread_on', scale, catalog);
            await page.evaluate(() => localStorage.removeItem('4chan-tw-timestamp'));
            await refresh.click();
            await expect.poll(() => pending.length).toBe(1);
            await expect(panel).toHaveAttribute('aria-busy', 'true');
            await expect(refresh).toBeDisabled();
            await expectIcon(refresh, family, 'post_expand_rotate', scale, catalog);
            await pending.shift().fulfill({ status: 503, body: 'Owned fixture unavailable' });
            await expect(refresh).toBeEnabled();
            await expect(panel).toHaveAttribute('aria-busy', 'false');
            await expectIcon(refresh, family, 'refresh', scale, catalog);
          }
        }
      });
    }
  });
}
