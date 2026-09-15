import { test as base, expect } from '@playwright/test';
const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  owned: async ({ context }, use) => {
    const request = context.request, password = 'owned-notifications-password';
    const write = form => request.post('/demo/post', { headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password } });
    const response = await write({ resto: '0', sub: 'Owned notifications', com: 'Tracked original post', track: '1' });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/thread\/(\d+)/)[1];
    const remove = () => request.post('/demo/delete', { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } });
    try { await use({ id, url: `/demo/thread/${id}`, path: `/_watch/demo/thread/${id}/posts`, remove,
      reply: async com => { const result = await write({ resto: id, com }); expect(result.status()).toBe(303); return result.headers().location.match(/#p(\d+)/)[1]; } }); }
    finally { await remove(); }
  },
});
const icon = page => page.locator('link[rel="shortcut icon"]');
const auto = page => page.locator('.threadNav.desktop input[data-cmd="auto"]').first();
const sound = page => page.locator('.threadNav.desktop input[data-cmd="sound"]').first();
const status = page => page.locator('.threadNav.desktop .nativeUpdaterStatus').first();
const update = page => page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
async function initialize(page, owned, settings = {}) {
  await page.addInitScript(settings => localStorage.setItem('4chan-settings', JSON.stringify(settings)), settings);
  await page.setViewportSize({ width: 1280, height: 400 }); await page.goto(owned.url);
  await expect.poll(() => page.evaluate(id => JSON.parse(localStorage.getItem(`4chan-track-demo-${id}`) || '{}')[`>>${id}`], owned.id)).toBe(1);
  const time = new Date('2026-09-13T19:00:00Z'); await page.clock.install({ time }); await page.clock.pauseAt(time);
}
async function tick(page, seconds = 10) { await page.clock.runFor(seconds * 1000); }

test('real posting receipts decorate initial and appended quote text once without changing navigation', async ({ page, context, owned }) => {
  const existing = await owned.reply(`>>${owned.id}`);
  await initialize(page, owned);
  const quote = page.locator(`#m${existing} .quotelink`);
  await expect(quote).toHaveText(`>>${owned.id} (You) (OP)`); await expect(quote).toHaveClass(/ql-tracked/);
  await expect(quote).toHaveAttribute('href', `/demo/post/${owned.id}`);
  const next = await owned.reply(`>>${owned.id}\n>>999999999`); await update(page);
  await expect(status(page)).toHaveText('1 new post');
  await expect(page.locator(`#m${next} .quotelink`)).toHaveText([`>>${owned.id} (You) (OP)`, '>>999999999 →']);
  await tick(page, 1); await update(page); await expect(status(page)).toHaveText('No new posts');
  await expect(quote).toHaveText(`>>${owned.id} (You) (OP)`);
  const other = await context.newPage(); await other.goto(owned.url);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await expect(quote).toHaveText(`>>${owned.id}`); await expect(quote).not.toHaveClass(/ql-tracked/);
  await other.evaluate(() => localStorage.setItem('4chan-settings', '{}'));
  await expect(quote).toHaveText(`>>${owned.id} (You) (OP)`);
  await page.goto('/demo/'); await expect(page.locator('.ql-tracked')).toHaveCount(0);
});

test('actual filter results and tracked replies drive favicon priority until stopping or reading', async ({ page, owned }) => {
  await owned.reply('Existing reply'); await initialize(page, owned, { filter: true });
  await page.evaluate(() => localStorage.setItem('4chan-filters', JSON.stringify([{ active: true, pattern: 'Highlight me', boards: 'demo', type: 2, color: '#ff0000' }])));
  await auto(page).check();
  await owned.reply('Ordinary new reply'); await tick(page); await expect(status(page)).toHaveText('10');
  await expect(icon(page)).toHaveAttribute('href', '/static/notifications/favicon-ws-newposts.ico');
  const highlighted = await owned.reply('Highlight me'); await tick(page); await expect(status(page)).toHaveText('10');
  await expect(page.locator(`#p${highlighted}`)).toHaveClass(/filter-hl/);
  await expect(icon(page)).toHaveAttribute('href', '/static/notifications/favicon-ws-newfilters.ico');
  const quoted = await owned.reply(`>>${owned.id}\nHighlight me`); await tick(page); await expect(status(page)).toHaveText('10');
  await expect(page.locator(`#p${quoted}`)).toHaveClass(/filter-hl/);
  await expect(icon(page)).toHaveAttribute('href', '/static/notifications/favicon-ws-newreplies.ico');
  await owned.reply('Highlight me again'); await tick(page); await expect(status(page)).toHaveText('10');
  await expect(icon(page)).toHaveAttribute('href', '/static/notifications/favicon-ws-newreplies.ico');
  await auto(page).uncheck(); await expect(icon(page)).toHaveAttribute('href', '/static/notifications/favicon-ws.ico');
  await expect(page).toHaveTitle(/^\(4\)/);
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight)); await tick(page, .1);
  await expect(page).not.toHaveTitle(/^\(/);
});

for (const linkify of [false, true]) {
  test(`updater completion waits for the final filter generation with linkification ${linkify ? 'enabled' : 'disabled'}`, async ({ page, request, owned }) => {
    await owned.reply('Existing reply');
    await initialize(page, owned, { filter: true, linkify });
    await page.evaluate(() => localStorage.setItem('4chan-filters', JSON.stringify([
      { active: true, pattern: 'Final filter generation', boards: 'demo', type: 2, color: '#ff0000' },
    ])));
    const reply = await owned.reply('Final filter generation https://lower.test/path then HTTPS://UPPER.TEST/Path?Q=One');
    const snapshot = await (await request.get(owned.path)).json();
    const html = snapshot.posts.find(post => post.no === reply).html;
    expect(html).toContain('href="https://lower.test/path"');
    expect(html).toContain('HTTPS://UPPER.TEST/Path?Q=One');
    expect(html).not.toContain('data-native-linkified');
    await page.evaluate(id => {
      window.completedFilterGeneration = null;
      document.addEventListener('4chanThreadUpdated', () => {
        const post = document.getElementById(`p${id}`);
        window.completedFilterGeneration = {
          highlighted: post.classList.contains('filter-hl'),
          generated: post.querySelectorAll('a[data-native-linkified]').length,
          notice: document.querySelector('.nativeFilterNotice').textContent,
        };
      }, { once: true });
    }, reply);
    // A default-disabled watcher returns from acknowledgement immediately. No
    // watcher-lock wait may accidentally hide the filter cancellation race.
    await update(page);
    await expect(status(page)).toHaveText('1 new post');
    expect(await page.evaluate(() => window.completedFilterGeneration)).toEqual({
      highlighted: true, generated: linkify ? 1 : 0, notice: '',
    });
    await expect(page.locator(`#p${reply}`)).toHaveClass(/filter-hl/);
  });
}

test('manual updates retain the default icon and terminal archival and deletion select fixed dead icons', async ({ page, request, owned }) => {
  await initialize(page, owned); await owned.reply(`>>${owned.id}`); await update(page);
  await expect(status(page)).toHaveText('1 new post');
  await expect(icon(page)).toHaveAttribute('href', '/static/notifications/favicon-ws.ico');
  const snapshot = await (await request.get(owned.path)).json(); snapshot.archived = true;
  await page.route(`**${owned.path}`, route => route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) }));
  await tick(page, 1); await update(page); await expect(status(page)).toHaveText('This thread is archived');
  await expect(icon(page)).toHaveAttribute('href', '/static/notifications/favicon-ws-deadthread.ico');
  await page.unrouteAll(); await page.reload(); await owned.remove(); await update(page);
  await expect(status(page)).toHaveText('This thread has been pruned or deleted');
  await expect(icon(page)).toHaveAttribute('href', '/static/notifications/favicon-ws-deadthread.ico');
});

test('opted-in hidden reply notifications play real local audio and playback rejection cannot stop updates', async ({ page, owned }) => {
  await initialize(page, owned, { updaterSound: true });
  await expect(sound(page)).not.toBeChecked();
  await page.evaluate(() => {
    window.plays = []; const play = HTMLMediaElement.prototype.play;
    HTMLMediaElement.prototype.play = function () {
      window.plays.push(this.src);
      if (window.rejectSound) return Promise.reject(new DOMException('Playback blocked', 'NotAllowedError'));
      const promise = play.call(this); promise.then(() => { window.audioPlayed = true; }, () => {}); return promise;
    };
  });
  await sound(page).check(); await expect(page.locator('input[data-cmd="sound"]').last()).toBeChecked();
  await auto(page).check();
  await page.evaluate(() => Object.defineProperty(document, 'hidden', { configurable: true, value: true }));
  const first = await owned.reply(`>>${owned.id}`); await tick(page);
  await expect(page.locator(`#p${first}`)).toBeAttached(); await expect(status(page)).toHaveText('60');
  await expect.poll(() => page.evaluate(() => window.audioPlayed)).toBe(true);
  expect(await page.evaluate(() => window.plays)).toEqual([`${origin}/static/notifications/beep.ogg`]);
  await page.evaluate(() => { window.rejectSound = true; });
  await owned.reply(`>>${owned.id}\nPlayback refusal`); await tick(page, 60);
  await expect(status(page)).toHaveText('60'); expect(await page.evaluate(() => window.plays.length)).toBe(2);
  await page.evaluate(() => Object.defineProperty(document, 'hidden', { configurable: true, value: false }));
  await owned.reply(`>>${owned.id}\nVisible reply`); await tick(page, 60); await expect(status(page)).toHaveText('10');
  expect(await page.evaluate(() => window.plays.length)).toBe(2);
  await page.reload(); await expect(sound(page)).not.toBeChecked();
});

test('the media CSP permits the fixed beep and blocks the same playable bytes on a healthy denied origin', async ({ page, owned }) => {
  await initialize(page, owned);
  const allowed = `${origin}/static/notifications/beep.ogg`, denied = new URL(allowed);
  denied.hostname = 'localhost';
  const positive = await page.request.get(allowed), healthy = await page.request.get(denied.href);
  expect(positive.status()).toBe(200); expect(healthy.status()).toBe(200);
  expect(healthy.headers()['content-type']).toBe('audio/ogg');
  expect(await healthy.body()).toEqual(await positive.body());
  const result = await page.evaluate(async denied => {
    const beep = new Audio('/static/notifications/beep.ogg');
    const loaded = new Promise(resolve => { beep.oncanplaythrough = () => resolve(true); beep.onerror = () => resolve(false); });
    beep.load(); const allowed = await loaded;
    const violation = new Promise(resolve => document.addEventListener('securitypolicyviolation', event => {
      if (event.effectiveDirective === 'media-src') resolve({ directive: event.effectiveDirective, blocked: event.blockedURI });
    }));
    const forbidden = new Audio(denied); forbidden.load();
    return { allowed, violation: await violation };
  }, denied.href);
  expect(result).toEqual({ allowed: true, violation: { directive: 'media-src', blocked: denied.href } });
});
