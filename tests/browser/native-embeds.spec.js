import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { test as base, expect } from '@playwright/test';
import { openWatcherSettings } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';
const youtubeId = 'dQw4w9WgXcQ';
const youtube = `https://www.youtube.com/watch?v=${youtubeId}&t=90s`;
const soundcloud = 'https://soundcloud.com/forss/flickermood';

const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-native-embeds-password';
    const write = form => withPostingHistory(() => request.post('/demo/post', {
      headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password },
    }));
    const response = await write({ resto: '0', sub: 'Owned native embeds', com: `${youtube}\n${soundcloud}` });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/thread\/(\d+)/)[1];
    try { await use({ id, url: `/demo/thread/${id}`, reply: async com => {
      const result = await write({ resto: id, com });
      expect(result.status(), await result.text()).toBe(303);
      return result.headers().location.match(/#p(\d+)/)[1];
    } }); }
    finally {
      await withDeletionQuota(async () => {
        await request.post('/demo/delete', {
          headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
        });
      });
    }
  },
});

test('provider text remains text and frames load only after the real Embed controls are selected', async ({ page, owned }) => {
  const quoted = await owned.reply(`>>${owned.id}\n${Array.from({ length: 45 }, (_, index) => `Owned preview spacing line ${index}`).join('\n')}`);
  const providerRequests = [];
  page.on('request', request => {
    const url = new URL(request.url());
    if (['https://www.youtube-nocookie.com', 'https://w.soundcloud.com', 'https://i1.ytimg.com'].includes(url.origin)
      || (url.origin === 'https://soundcloud.com' && url.pathname === '/oembed')) {
      providerRequests.push({ url: url.href, headers: request.headers() });
    }
  });
  await page.route('https://www.youtube-nocookie.com/**', route => route.abort());
  await page.route('https://w.soundcloud.com/**', route => route.abort());
  await page.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ embedSoundCloud: true, threadStats: false }));
  });
  await page.goto(owned.url);

  const message = page.locator(`#m${owned.id}`);
  const youtubeLink = message.getByText(youtube, { exact: true });
  const soundCloudLink = message.getByText(soundcloud, { exact: true });
  await expect(youtubeLink).toHaveCount(1);
  await expect(soundCloudLink).toHaveCount(1);
  await expect(message.locator('a.linkified')).toHaveCount(0);
  expect(await youtubeLink.evaluate(node => node.tagName)).toBe('SPAN');
  expect(await soundCloudLink.evaluate(node => node.tagName)).toBe('SPAN');
  await expect(youtubeLink.locator('xpath=following-sibling::*[1]')).toHaveClass(/nativeEmbedControls/);
  await expect(soundCloudLink.locator('xpath=following-sibling::*[1]')).toHaveClass(/nativeEmbedControls/);
  await expect(message.locator('iframe')).toHaveCount(0);
  expect(providerRequests).toEqual([]);

  const quote = page.locator(`#m${quoted} .quotelink`).first();
  await quote.evaluate(node => node.scrollIntoView({ block: 'start' }));
  await page.evaluate(() => scrollBy(0, -40));
  await quote.hover();
  await expect(page.locator('#quote-preview .postMessage')).toContainText(youtube);
  await expect(page.locator('#quote-preview .postMessage')).toContainText(soundcloud);
  await expect(page.locator('#quote-preview .nativeEmbedControls, #quote-preview iframe')).toHaveCount(0);
  expect(providerRequests).toEqual([]);

  await youtubeLink.hover();
  await page.waitForTimeout(50);
  expect(providerRequests).toEqual([]);
  await expect(page.locator('#yt-preview')).toHaveCount(0);

  const youtubeRequest = page.waitForRequest(request => new URL(request.url()).origin === 'https://www.youtube-nocookie.com');
  await youtubeLink.locator('xpath=following-sibling::*[1]').getByRole('link', { name: 'Embed', exact: true }).click();
  const requestedYouTube = await youtubeRequest;
  const youtubeFrame = message.locator('.nativeMediaEmbedYouTube iframe');
  await expect(youtubeFrame).toHaveAttribute('src', `https://www.youtube-nocookie.com/embed/${youtubeId}?start=90`);
  await expect(youtubeFrame).toHaveAttribute('referrerpolicy', 'strict-origin-when-cross-origin');
  expect(requestedYouTube.headers().referer).toBe(`${origin}/`);
  expect(providerRequests.map(item => new URL(item.url).origin)).toEqual(['https://www.youtube-nocookie.com']);

  await message.locator('.nativeMediaEmbedYouTube').locator('xpath=preceding-sibling::*[1]').getByRole('link', { name: 'Remove', exact: true }).click();
  await expect(youtubeFrame).toHaveCount(0);

  const soundCloudRequest = page.waitForRequest(request => new URL(request.url()).origin === 'https://w.soundcloud.com');
  await soundCloudLink.locator('xpath=following-sibling::*[1]').getByRole('link', { name: 'Embed', exact: true }).click();
  await soundCloudRequest;
  const soundCloudFrame = message.locator('.nativeMediaEmbedSoundCloud iframe');
  await expect(soundCloudFrame).toHaveAttribute('src', /^https:\/\/w\.soundcloud\.com\/player\/\?url=/);
  await expect(soundCloudFrame).toHaveAttribute('referrerpolicy', 'no-referrer');
  expect(providerRequests.map(item => new URL(item.url).origin)).toEqual([
    'https://www.youtube-nocookie.com', 'https://w.soundcloud.com',
  ]);

  await page.evaluate(() => {
    const current = JSON.parse(localStorage.getItem('4chan-settings'));
    localStorage.setItem('4chan-settings', JSON.stringify({ ...current, disableAll: true }));
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
  });
  await expect(message.locator('.nativeEmbedControls,iframe')).toHaveCount(0);
  await expect(message.locator('a,span')).toHaveCount(0);
  await expect(message).toHaveText(`${youtube}${soundcloud}`);
});

test('desktop linkified provider destinations decode source entities without changing visible text', async ({ page, owned }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ linkify: true, threadStats: false })));
  await page.goto(owned.url);
  const message = page.locator(`#m${owned.id}`);
  const link = message.getByRole('link', { name: youtube, exact: true });
  await expect(link).toHaveAttribute('href', `/derefer?url=${encodeURIComponent(youtube.replace('&', '&amp;'))}`);
  await expect(link.locator('xpath=following-sibling::*[1]')).toHaveClass(/nativeEmbedControls/);
  await expect(message.locator('iframe')).toHaveCount(0);
});

test('mobile source behavior exposes YouTube Open without creating or preloading a player', async ({ page, owned }) => {
  const providerRequests = [];
  page.on('request', request => {
    const url = new URL(request.url());
    if (['https://www.youtube-nocookie.com', 'https://w.soundcloud.com', 'https://i1.ytimg.com'].includes(url.origin)
      || (url.origin === 'https://soundcloud.com' && url.pathname === '/oembed')) providerRequests.push(url.href);
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ embedYouTube: false, embedSoundCloud: false, threadStats: false }));
  });
  await page.goto(owned.url);
  const message = page.locator(`#m${owned.id}`), youtubeLink = message.getByRole('link', { name: youtube, exact: true });
  const control = youtubeLink.locator('xpath=following-sibling::*[1]');
  await expect(control).toHaveClass(/nativeEmbedControls/);
  await expect(control.getByRole('link', { name: 'Open', exact: true })).toHaveAttribute('href', youtube);
  await expect(message.locator(`a[href="${soundcloud}"] + .nativeEmbedControls`)).toHaveCount(0);
  await expect(message.locator('iframe')).toHaveCount(0);
  await youtubeLink.hover();
  await page.waitForTimeout(50);
  expect(providerRequests).toEqual([]);

  const settings = await openWatcherSettings(page);
  await expect(settings.getByLabel('Embed YouTube links', { exact: true })).toHaveCount(0);
  await expect(settings.getByLabel('Embed SoundCloud links', { exact: true })).toHaveCount(0);
  await settings.getByRole('button', { name: 'Close settings' }).click();
});
