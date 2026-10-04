import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const reference = JSON.parse(await readFile(new URL('../../apps/public/tests/fixtures/custom-spoilers.json', import.meta.url)));
for (const device of [
  { name: 'desktop', viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 },
  { name: 'mobile density 2', viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
]) {
  test.describe(device.name, () => {
    test.use(device);
    for (const slug of ['m', 'news', 'vm', 'vst', 's4s']) {
      test(`${slug} retains source server choices and catalog suffix through reveal and hide`, async ({ page }) => {
        const row = reference.boards.find(row => row.board === slug);
        const allowed = row.source_html_urls.map(url => '/static/catalog/' + url.split('/').at(-1));
        await page.goto(`/${slug}/`);
        const thumb = page.locator('.imgspoiler img').first();
        await expect(thumb).toBeVisible();
        expect(allowed).toContain(await thumb.getAttribute('src'));
        await expect(page.locator('[data-custom-spoiler]').first()).toHaveAttribute('data-custom-spoiler', String(row.count));
        await expect(thumb).toHaveJSProperty('naturalWidth', 100);
        await page.goto(`/${slug}/catalog`);
        const hidden = `/static/catalog/spoiler-${slug}${row.count}.png`;
        const card = page.locator('#thumb-1000205');
        await expect(card).toHaveAttribute('src', hidden);
        await expect(card).toHaveAttribute('data-spoiler-placeholder', hidden);
        if (['news', 'vm'].includes(slug)) {
          const response = await page.request.get(hidden);
          expect(response.status()).toBe(404);
          await expect(card).toHaveJSProperty('naturalWidth', 0);
        } else { await expect(card).toHaveJSProperty('naturalWidth', 100); }
        let navigations = 0;
        page.on('request', request => { if (request.isNavigationRequest()) navigations++; });
        await page.locator('#catalog-spoilers').selectOption('on');
        await expect(card).toHaveAttribute('src', `http://localhost:3004/${slug}/1000205s.jpg`);
        await expect(card).toHaveJSProperty('naturalWidth', 250);
        await page.locator('#catalog-spoilers').selectOption('off');
        await expect(card).toHaveAttribute('src', hidden);
        expect(navigations).toBe(0);
        await expect(page.locator('.fileDeleted')).toHaveAttribute('src', '/static/catalog/filedeleted-res.gif');
      });
    }
    test('native previews reuse primary choices and cache cross-board choices through reveal and conceal', async ({ page, context }) => {
      const remote = await context.newPage();
      const trees = [];
      for (const slug of ['news', 'vm', 'vst', 's4s']) {
        await remote.goto(`/${slug}/`);
        trees.push(await remote.evaluate(async slug => {
          const native = await import('/static/native-filter.v1.js');
          const context = { origin: location.origin, board: slug, thread: '1000201', mediaOrigin: 'http://localhost:3004' };
          return { slug, tree: native.localQuoteTree(document.getElementById('pc1000205'), context, '1000205') };
        }, slug));
      }
      await remote.close();
      await page.goto('/m/');
      await expect(page.locator('.postMenuBtn').first()).toBeAttached();
      const primary = await page.locator('.imgspoiler img').first().getAttribute('src');
      const result = await page.evaluate(async trees => {
        const native = await import('/static/native-filter.v1.js');
        const images = await import('/static/native-images.v1.js');
        const context = board => ({ origin: location.origin, board, thread: '1000201', mediaOrigin: 'http://localhost:3004' });
        const holder = document.createElement('div'); document.body.append(holder);
        const recipe = native.localQuoteTree(document.getElementById('pc1000205'), context('m'), '1000205');
        const create = (tree, board) => {
          const post = native.prepareQuotePost(tree, context(board), '1000205').build(document);
          holder.append(post); return post;
        };
        const local = create(recipe, 'm');
        const choices = trees.map(({ tree, slug }) => {
          const first = create(tree, slug).querySelector('.imgspoiler img').getAttribute('src');
          const changed = structuredClone(tree); delete changed.attrs['data-custom-spoiler'];
          const second = create(changed, slug).querySelector('.imgspoiler img').getAttribute('src');
          return { slug, first, second };
        });
        let config = { revealSpoilers: false };
        const manager = images.mountNativeImages({ root: holder, mediaOrigin: 'http://localhost:3004', settings: () => config });
        const hidden = local.querySelector('.imgspoiler img').getAttribute('src');
        config.revealSpoilers = true; manager.refresh();
        const revealed = local.querySelector('.fileThumb:not(.imgspoiler) img')?.getAttribute('src');
        config.revealSpoilers = false; manager.refresh();
        const concealed = local.querySelector('.imgspoiler img').getAttribute('src');
        manager.dispose(); holder.remove();
        return { hidden, revealed, concealed, choices };
      }, trees);
      expect(result.hidden).toBe(primary); expect(result.concealed).toBe(primary);
      expect(result.revealed).toBe('http://localhost:3004/m/1000205s.jpg');
      for (const choice of result.choices) {
        expect(choice.second).toBe(choice.first);
        const count = reference.boards.find(row => row.board === choice.slug).count;
        expect(Array.from({ length: count }, (_, i) => `/static/catalog/spoiler-${choice.slug}${i + 1}.png`)).toContain(choice.first);
      }
    });
  });
}
