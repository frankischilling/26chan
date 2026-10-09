import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { assertSubtitleDocument } from './source-board-subtitles-contract.mjs';
import { canonicalSubtitleBoards, subtitlePaths, withOwnedSubtitles } from './source-board-subtitles-fixture.mjs';

const chrome = JSON.parse(await readFile(new URL('../../docs/public-page-chrome-reference.json', import.meta.url)));
const origin = 'http://127.0.0.1:3000';
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];

test('HTTP persisted canonical source boards and every owned mode have exact subtitle markup', async ({ request }) => {
  for (const board of canonicalSubtitleBoards()) for (const mode of ['index', 'catalog']) {
    const response = await request.get(`/${board.slug}/${mode === 'catalog' ? 'catalog' : ''}`);
    expect(response.status()).toBe(200);
    assertSubtitleDocument(await response.text(), board, mode);
  }
  await withOwnedSubtitles(async ({ boards }) => {
    for (const board of boards) for (const [mode, path] of subtitlePaths(board)) {
      const response = await request.get(path); expect(response.status()).toBe(200);
      assertSubtitleDocument(await response.text(), board, mode);
    }
  });
});

for (const javaScriptEnabled of [true, false]) test.describe(`JavaScript ${javaScriptEnabled ? 'on' : 'off'}`, () => {
  test.use({ javaScriptEnabled });
  test('persisted subtitles survive reload in every page mode without executing board metadata', async ({ page }) => {
    const external = [], errors = [];
    page.on('request', request => { if (new URL(request.url()).origin !== origin) external.push(request.url()); });
    page.on('pageerror', error => errors.push(error.message));
    await withOwnedSubtitles(async ({ boards }) => {
      for (const board of boards) for (const [mode, path] of subtitlePaths(board)) {
        const response = await page.goto(path); expect(response.status()).toBe(200);
        assertSubtitleDocument(await page.content(), board, mode);
        if (board.kind !== 'none') await expect(page.locator('.boardSubtitle')).toBeVisible();
        if (board.kind === 'fiction') await expect(page.locator('.boardSubtitle br')).toHaveCount(1);
        if (board.kind === 'worksafe_gif') {
          const link = page.locator('.boardSubtitle a');
          await expect(link).toHaveAttribute('href', '/wsg/');
          await expect(link).toHaveAttribute('title', 'Worksafe GIF');
          await link.focus(); await expect(link).toBeFocused();
        }
        expect(await page.evaluate(() => window.subtitleInjected)).toBeUndefined();
        await page.reload(); assertSubtitleDocument(await page.content(), board, mode);
      }
      // Follow the real source destination once, including without JavaScript.
      const gif = boards.find(board => board.kind === 'worksafe_gif');
      await page.goto(`/${gif.slug}/`);
      await page.locator('.boardSubtitle a').focus(); await page.keyboard.press('Enter');
      await expect(page).toHaveURL(`${origin}/wsg/`);
      await expect(page.locator('.boardTitle')).toContainText('/wsg/');
    });
    expect(errors).toEqual([]); expect(external).toEqual([]);
  });
});

for (const theme of themes) for (const scale of [1, 2]) test.describe(`${theme}, DPR ${scale}`, () => {
  test.use({ deviceScaleFactor: scale });
  test('persisted subtitle styles and line geometry match the pinned public components', async ({ page, context }) => {
    await context.addCookies([
      { name: 'board-theme', value: theme, url: origin },
      { name: 'board-theme-ws', value: theme, url: origin },
    ]);
    await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
    await withOwnedSubtitles(async ({ boards }) => {
      // Fiction is tested in both safety modes; TEXT_ONLY remains configured on
      // the worksafe fixture so its layout must retain the same subtitle policy.
      for (const board of boards.filter(board => board.kind === 'fiction')) {
        for (const width of [1280, 481, 480, 390]) for (const mode of ['index', 'catalog']) {
          await page.setViewportSize({ width, height: 900 });
          await page.goto(`/${board.slug}/${mode === 'catalog' ? 'catalog' : ''}`);
          await expect(page.locator('body')).toHaveAttribute('data-native-never-mobile', 'false');
          const recorded = chrome.cases.find(row => row.theme === theme && row.scale === scale
            && row.width === width && row.mode === mode && row.worksafe === board.worksafe);
          expect(recorded).toBeTruthy();
          const expected = chrome.component_styles[recorded.styles.subtitle];
          const actual = await page.locator('.boardSubtitle').evaluate((element, expected) => {
            const computed = getComputedStyle(element), box = element.getBoundingClientRect();
            const banner = element.parentElement.getBoundingClientRect();
            const title = element.previousElementSibling.getBoundingClientRect();
            const lineRects = [...element.childNodes].filter(node => node.nodeType === Node.TEXT_NODE).map(node => {
              const range = document.createRange(); range.selectNodeContents(node);
              return [...range.getClientRects()].map(rect => ({ top: rect.top, bottom: rect.bottom }));
            });
            return { styles: Object.fromEntries(Object.keys(expected).map(key => [key, computed[key]])),
              top: box.top, bottom: box.bottom, titleBottom: title.bottom, bannerBottom: banner.bottom,
              width: box.width, bannerWidth: banner.width, lineRects,
              overflow: document.documentElement.scrollWidth > innerWidth };
          }, expected);
          expect(actual.styles).toEqual(expected);
          expect(actual.top).toBeGreaterThanOrEqual(actual.titleBottom - 0.5);
          expect(actual.bottom).toBeLessThanOrEqual(actual.bannerBottom + 0.5);
          expect(actual.width).toBeCloseTo(actual.bannerWidth, 1);
          expect(actual.lineRects).toHaveLength(2);
          expect(actual.lineRects[1][0].top).toBeGreaterThanOrEqual(actual.lineRects[0].at(-1).bottom - 1);
          if (width === 1280) expect(actual.lineRects.map(lines => lines.length)).toEqual([1, 1]);
          expect(actual.overflow).toBe(false);
        }
      }
    });
  });
});
