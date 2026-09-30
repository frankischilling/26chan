import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { EMBED_LIMITS, embedTarget, soundCloudTarget, youtubeTarget } from '../../apps/public/static/native-embeds.v1.js';

const yt = 'dQw4w9WgXcQ';
const origin = 'https://embeds.example';

test('provider parsers accept finite canonical URLs and build only CSP-approved frame targets', () => {
  assert.deepEqual(youtubeTarget(`https://www.youtube.com/watch?v=${yt}`), {
    provider: 'youtube', id: yt, start: 0,
    source: `https://www.youtube.com/watch?v=${yt}`,
    embed: `https://www.youtube-nocookie.com/embed/${yt}`,
  });
  assert.equal(youtubeTarget(`https://youtube.com/watch?v=${yt}`).id, yt);
  assert.deepEqual(youtubeTarget(`https://youtu.be/${yt}?t=1m30s`), {
    provider: 'youtube', id: yt, start: 90,
    source: `https://youtu.be/${yt}?t=1m30s`,
    embed: `https://www.youtube-nocookie.com/embed/${yt}?start=90`,
  });
  assert.equal(youtubeTarget(`https://youtu.be/${yt}?t=61s`).start, 61);
  assert.deepEqual(youtubeTarget(`https://youtu.be/${yt}?si=owned-share&t=90s`), {
    provider: 'youtube', id: yt, start: 90,
    source: `https://youtu.be/${yt}?si=owned-share&t=90s`,
    embed: `https://www.youtube-nocookie.com/embed/${yt}?start=90`,
  });
  assert.deepEqual(youtubeTarget(`https://youtu.be/${yt}?si=owned-share`), {
    provider: 'youtube', id: yt, start: 0,
    source: `https://youtu.be/${yt}?si=owned-share`,
    embed: `https://www.youtube-nocookie.com/embed/${yt}`,
  });
  assert.deepEqual(youtubeTarget(`https://www.youtube.com/watch?list=PLowned&index=2&v=${yt}&t=90m&feature=share`), {
    provider: 'youtube', id: yt, start: 5400,
    source: `https://www.youtube.com/watch?list=PLowned&index=2&v=${yt}&t=90m&feature=share`,
    embed: `https://www.youtube-nocookie.com/embed/${yt}?start=5400`,
  });
  assert.deepEqual(youtubeTarget(`https://www.youtube.com/watch?v=${yt}&autoplay=1&origin=https%3A%2F%2Fevil.test%2F&t=90s`), {
    provider: 'youtube', id: yt, start: 90,
    source: `https://www.youtube.com/watch?v=${yt}&autoplay=1&origin=https%3A%2F%2Fevil.test%2F&t=90s`,
    embed: `https://www.youtube-nocookie.com/embed/${yt}?start=90`,
  });
  assert.equal(youtubeTarget(`https://youtu.be/${yt}#t=90s`).start, 90);
  for (const raw of [`https://youtu.be/${yt}?t=1h2m`, `https://youtu.be/${yt}?t=10081m`,
    `https://youtu.be/${yt}?t=90sfoo`]) {
    const target = youtubeTarget(raw);
    assert.equal(target.start, 0, raw);
    assert.equal(target.embed, `https://www.youtube-nocookie.com/embed/${yt}`, raw);
  }
  for (const value of [
    `http://www.youtube.com/watch?v=${yt}`,
    `https://www.youtube.com.evil.test/watch?v=${yt}`,
    `https://user@www.youtube.com/watch?v=${yt}`,
    `https://www.youtube.com:444/watch?v=${yt}`,
    `https://www.youtube.com/embed/${yt}`,
    `https://www.youtube.com/watch?v=${yt}&v=${yt}`,
    `https://www.youtube.com/watch?v=${yt}&t=90s&t=90m`,
    `https://www.youtube.com/watch?v=${yt}&t=90s#t=90m`,
    `https://youtu.be/${yt}/extra`,
    `https://youtu.be/${yt}?t=${'1'.repeat(2050)}`,
  ]) assert.equal(youtubeTarget(value), null, value);

  assert.equal(soundCloudTarget('https://soundcloud.com/forss/flickermood').embed.startsWith('https://w.soundcloud.com/player/?'), true);
  assert.equal(soundCloudTarget('https://soundcloud.com/artist').source, 'https://soundcloud.com/artist');
  assert.equal(soundCloudTarget('https://soundcloud.com/artist/sets/my-set').source, 'https://soundcloud.com/artist/sets/my-set');
  for (const value of [
    'http://soundcloud.com/forss/flickermood',
    'https://www.soundcloud.com/forss/flickermood',
    'https://snd.sc/example',
    'https://soundcloud.com.evil.test/forss/flickermood',
    'https://soundcloud.com/Forss/flickermood',
    'https://soundcloud.com/forss/flickermood/',
    'https://soundcloud.com/forss/flickermood?utm_source=test',
    'https://soundcloud.com/forss/flickermood#fragment',
    'https://soundcloud.com/forss/not-sets/extra',
    'https://soundcloud.com/a/b/c/d',
  ]) assert.equal(soundCloudTarget(value), null, value);
  assert.equal(embedTarget(`https://youtu.be/${yt}`).provider, 'youtube');
  assert.equal(embedTarget('https://soundcloud.com/forss/flickermood').provider, 'soundcloud');
  assert.equal(embedTarget('https://example.test/video'), null);
  assert.deepEqual(EMBED_LIMITS, { nodes: 32768, links: 4096, frames: 8, depth: 32 });
});

test('native embeds are click-only, projection-owned and cleared across disable, hiding and BFCache', async t => {
  const files = {};
  for (const name of ['native-embeds.v1.js', 'native-backlinks.v1.js']) {
    files[`/static/${name}`] = await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8');
  }
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup(config = {}) {
      const context = await browser.newContext({ viewport: { width: 1000, height: 700 } });
      const page = await context.newPage(), providerRequests = [], errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.route('**/*', async route => {
        const request = route.request(), url = new URL(request.url());
        if (files[url.pathname] && url.origin === origin) {
          return route.fulfill({ contentType: 'text/javascript', body: files[url.pathname] });
        }
        if (url.origin === origin && url.pathname === '/test/thread/1') {
          return route.fulfill({ contentType: 'text/html', body: `<!doctype html><html><body>
            <div class="board"><section class="thread"><article id="pc1"><div class="post" id="p1">
              <blockquote class="postMessage" id="m1">
                <a id="yt" href="https://www.youtube.com/watch?v=${yt}&amp;t=90s" rel="nofollow noreferrer noopener">https://www.youtube.com/watch?v=${yt}&amp;t=90s</a>
                <a id="sc" href="https://soundcloud.com/forss/flickermood" rel="nofollow noreferrer noopener">https://soundcloud.com/forss/flickermood</a>
              </blockquote>
            </div></article></section></div></body></html>` });
        }
        if (['https://www.youtube-nocookie.com', 'https://w.soundcloud.com', 'https://www.youtube.com',
          'https://youtube.com', 'https://soundcloud.com'].includes(url.origin)) {
          providerRequests.push({ url: url.href, headers: request.headers() });
          return route.abort();
        }
        if (url.pathname.endsWith('favicon.ico')) return route.fulfill({ status: 204 });
        return route.abort();
      });
      await page.goto(`${origin}/test/thread/1`);
      await page.evaluate(async initial => {
        window.config = initial;
        const ownership = await import('/static/native-backlinks.v1.js');
        const embeds = await import('/static/native-embeds.v1.js');
        window.projection = ownership.createCommentProjection();
        window.mountEmbeds = (limits = {}) => embeds.mountNativeEmbeds({
          root: document.querySelector('.board'), settings: () => config,
          hasMobileLayout: () => config.mobile === true, projection, limits,
        });
        window.embeds = mountEmbeds();
      }, config);
      return { context, page, providerRequests, errors };
    }

    await t.test('default desktop and opt-in SoundCloud make no provider request before click', async () => {
      const { context, page, providerRequests, errors } = await setup();
      try {
        assert.deepEqual(providerRequests, []);
        assert.equal(await page.locator('#yt + .nativeEmbedControls .nativeEmbedToggle').textContent(), 'Embed');
        assert.equal(await page.locator('#sc + .nativeEmbedControls').count(), 0);
        assert.equal(await page.locator('iframe').count(), 0);
        const original = await page.locator('#yt').evaluate(node => ({ href: node.getAttribute('href'), text: node.textContent, rel: node.rel }));
        assert.deepEqual(original, {
          href: `https://www.youtube.com/watch?v=${yt}&t=90s`,
          text: `https://www.youtube.com/watch?v=${yt}&t=90s`, rel: 'nofollow noreferrer noopener',
        });

        await page.evaluate(() => { config.embedSoundCloud = true; embeds.refresh(); });
        assert.equal(await page.locator('#sc + .nativeEmbedControls .nativeEmbedToggle').textContent(), 'Embed');
        assert.deepEqual(providerRequests, []);
        const projected = await page.evaluate(() => projection.html(document.querySelector('#m1')));
        assert.equal(projected.includes('nativeEmbed'), false);
        assert.equal(projected.includes('[Embed]'), false);
        assert.equal(projected.includes(`https://www.youtube.com/watch?v=${yt}&amp;t=90s`), true);

        await page.locator('#yt + .nativeEmbedControls .nativeEmbedToggle').click();
        await page.waitForFunction(() => document.querySelector('.nativeMediaEmbedYouTube iframe'));
        await page.waitForFunction(() => window.frames.length > 0);
        assert.equal(await page.locator('.nativeMediaEmbedYouTube iframe').getAttribute('src'),
          `https://www.youtube-nocookie.com/embed/${yt}?start=90`);
        assert.equal(await page.locator('.nativeMediaEmbedYouTube iframe').getAttribute('referrerpolicy'), 'strict-origin-when-cross-origin');
        assert.match(await page.locator('.nativeMediaEmbedYouTube iframe').getAttribute('sandbox'), /allow-scripts/);
        assert.equal(providerRequests.length, 1);
        assert.equal(providerRequests[0].headers.referer, `${origin}/`);

        await page.locator('#sc + .nativeEmbedControls .nativeEmbedToggle').click();
        await page.waitForFunction(() => document.querySelector('.nativeMediaEmbedSoundCloud iframe'));
        assert.equal(providerRequests.length, 2);
        assert.equal(new URL(providerRequests[1].url).origin, 'https://w.soundcloud.com');
        assert.equal(await page.locator('.nativeMediaEmbedSoundCloud iframe').getAttribute('referrerpolicy'), 'no-referrer');

        const projectedWithFrames = await page.evaluate(() => projection.html(document.querySelector('#m1')));
        assert.equal(projectedWithFrames, projected);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('mobile YouTube is Open-only and never creates a player', async () => {
      const { context, page, providerRequests, errors } = await setup({ mobile: true, embedYouTube: false });
      try {
        const open = page.locator('#yt + .nativeEmbedControls .nativeEmbedToggle');
        assert.equal(await open.textContent(), 'Open');
        assert.equal(await open.getAttribute('href'), `https://www.youtube.com/watch?v=${yt}&t=90s`);
        assert.equal(await page.locator('#sc + .nativeEmbedControls').count(), 0);
        assert.equal(await page.locator('iframe').count(), 0);
        assert.deepEqual(providerRequests, []); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('a stale mobile Open control revalidates its source before navigation', async () => {
      const { context, page, providerRequests, errors } = await setup({ mobile: true, embedYouTube: false });
      try {
        const changed = 'AAAAAAAAAAA';
        const before = page.url();
        await page.evaluate(id => {
          const anchor = document.querySelector('#yt');
          const stale = anchor.nextElementSibling.querySelector('.nativeEmbedToggle');
          anchor.href = `https://www.youtube.com/watch?v=${id}`;
          stale.click();
        }, changed);
        const open = page.locator('#yt + .nativeEmbedControls .nativeEmbedToggle');
        assert.equal(await open.getAttribute('href'), `https://www.youtube.com/watch?v=${changed}`);
        assert.equal(page.url(), before);
        assert.deepEqual(providerRequests, []);

        await page.evaluate(() => {
          const anchor = document.querySelector('#yt');
          const stale = anchor.nextElementSibling.querySelector('.nativeEmbedToggle');
          document.querySelector('#p1').classList.add('post-hidden');
          stale.click();
        });
        assert.equal(await page.locator('#yt + .nativeEmbedControls').count(), 0);
        assert.equal(page.url(), before);
        assert.deepEqual(providerRequests, []);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('native-linkified derefer anchors qualify only when label and destination agree', async () => {
      const { context, page, providerRequests, errors } = await setup();
      try {
        await page.evaluate(id => {
          const message = document.querySelector('#m1');
          const good = document.createElement('a');
          good.id = 'linked-good'; good.className = 'linkified'; good.dataset.nativeLinkified = 'true';
          good.href = `/derefer?url=${encodeURIComponent(`https://youtu.be/${id}`)}`;
          good.textContent = `https://youtu.be/${id}`;
          const bad = document.createElement('a');
          bad.id = 'linked-bad'; bad.className = 'linkified'; bad.dataset.nativeLinkified = 'true';
          bad.href = `/derefer?url=${encodeURIComponent(`https://youtu.be/${id}`)}`;
          bad.textContent = `https://www.youtube.com/watch?v=${id}`;
          message.append(document.createTextNode(' '), good, document.createTextNode(' '), bad);
        }, yt);
        await page.waitForFunction(() => document.querySelector('#linked-good + .nativeEmbedControls'));
        assert.equal(await page.locator('#linked-good + .nativeEmbedControls').count(), 1);
        assert.equal(await page.locator('#linked-bad + .nativeEmbedControls').count(), 0);
        assert.deepEqual(providerRequests, []); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('hidden, detached and disabled sources remove frames and controls without reopening them', async () => {
      const { context, page, providerRequests, errors } = await setup({ embedSoundCloud: true });
      try {
        await page.locator('#yt + .nativeEmbedControls .nativeEmbedToggle').click();
        await page.waitForFunction(() => document.querySelector('.nativeMediaEmbedYouTube iframe'));
        const before = providerRequests.length;
        await page.evaluate(() => document.querySelector('#p1').classList.add('post-hidden'));
        await page.waitForFunction(() => !document.querySelector('.nativeEmbedControls'));
        assert.equal(await page.locator('iframe').count(), 0);
        await page.evaluate(() => document.querySelector('#p1').classList.remove('post-hidden'));
        await page.waitForFunction(() => document.querySelectorAll('.nativeEmbedControls').length === 2);
        assert.equal(await page.locator('iframe').count(), 0);
        assert.equal(providerRequests.length, before);

        await page.locator('#sc + .nativeEmbedControls .nativeEmbedToggle').click();
        await page.waitForFunction(() => document.querySelector('.nativeMediaEmbedSoundCloud iframe'));
        await page.evaluate(() => {
          window.detachedFrame = document.querySelector('.nativeMediaEmbedSoundCloud iframe');
          document.querySelector('#pc1').remove();
        });
        await page.waitForFunction(() => !detachedFrame.isConnected && detachedFrame.getAttribute('src') === null);

        await page.evaluate(() => {
          config.disableAll = true; embeds.refresh();
        });
        assert.equal(await page.locator('.nativeEmbedControls,iframe').count(), 0);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('projection-owned synthetic copies stay undecorated and scan/frame bounds fail closed', async () => {
      const { context, page, providerRequests, errors } = await setup({ embedSoundCloud: true });
      try {
        await page.evaluate(() => {
          const source = document.querySelector('#m1');
          const copy = projection.clone(source); copy.id = 'synthetic-copy';
          projection.claim(copy, { kind: 'synthetic-copy' });
          document.querySelector('.thread').append(copy);
        });
        await new Promise(resolve => setTimeout(resolve, 30));
        assert.equal(await page.locator('#synthetic-copy .nativeEmbedControls').count(), 0);
        assert.equal(await page.locator('#m1 .nativeEmbedControls').count(), 2);

        await page.evaluate(() => { embeds.destroy(); embeds = mountEmbeds({ frames: 1 }); });
        await page.locator('#yt + .nativeEmbedControls .nativeEmbedToggle').click();
        await page.waitForFunction(() => document.querySelectorAll('iframe').length === 1);
        await page.locator('#sc + .nativeEmbedControls .nativeEmbedToggle').click();
        await new Promise(resolve => setTimeout(resolve, 30));
        assert.equal(await page.locator('iframe').count(), 1);

        await page.evaluate(() => { embeds.destroy(); embeds = mountEmbeds({ links: 1 }); });
        assert.equal(await page.locator('.nativeEmbedControls,iframe').count(), 0);
        assert.deepEqual(errors, []);
        assert.equal(providerRequests.length, 1);
      } finally { await context.close(); }
    });

    await t.test('replacement mounts and persisted history restore exactly one fresh control set', async () => {
      const { context, page, providerRequests, errors } = await setup({ embedSoundCloud: true });
      try {
        assert.equal(await page.locator('.nativeEmbedControls').count(), 2);
        await page.evaluate(() => { window.firstEmbeds = embeds; embeds = mountEmbeds(); firstEmbeds.destroy(); });
        assert.equal(await page.locator('.nativeEmbedControls').count(), 2);
        await page.locator('#yt + .nativeEmbedControls .nativeEmbedToggle').click();
        await page.waitForFunction(() => document.querySelector('iframe'));
        const before = providerRequests.length;
        await page.evaluate(() => {
          window.staleEmbedToggle = document.querySelector('#yt + .nativeEmbedControls .nativeEmbedToggle');
          window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
        });
        assert.equal(await page.locator('.nativeEmbedControls,iframe').count(), 0);
        await page.evaluate(() => staleEmbedToggle.click());
        await new Promise(resolve => setTimeout(resolve, 30));
        assert.equal(await page.locator('iframe').count(), 0);
        assert.equal(providerRequests.length, before);
        await page.evaluate(() => {
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
        });
        assert.equal(await page.locator('.nativeEmbedControls').count(), 2);
        assert.equal(await page.locator('iframe').count(), 0);
        assert.equal(providerRequests.length, before);
        await page.evaluate(() => {
          window.destroyedEmbedToggle = document.querySelector('#yt + .nativeEmbedControls .nativeEmbedToggle');
          embeds.destroy(); destroyedEmbedToggle.click();
          document.dispatchEvent(new Event('4chanSettingsSaved'));
        });
        await new Promise(resolve => setTimeout(resolve, 30));
        assert.equal(await page.locator('.nativeEmbedControls,iframe').count(), 0);
        assert.equal(providerRequests.length, before);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });
  } finally { await browser.close(); }
});
