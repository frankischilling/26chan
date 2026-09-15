import { test as base, expect } from '@playwright/test';
import { openWatcherSettings, saveWatcherSettings } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-linkification-password';
    const write = form => request.post('/demo/post', {
      headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password },
    });
    const response = await write({ resto: '0', sub: 'Owned linkification', com: 'Initial https://initial.test/path' });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/thread\/(\d+)/)[1];
    const remove = () => request.post('/demo/delete', {
      headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
    });
    try {
      await use({
        id, url: `/demo/thread/${id}`, path: `/_watch/demo/thread/${id}/posts`,
        reply: async com => {
          const result = await write({ resto: id, com });
          expect(result.status()).toBe(303);
          return result.headers().location.match(/#p(\d+)/)[1];
        },
      });
    } finally { await remove(); }
  },
});

const generated = 'a.linkified[data-native-linkified="true"]';
const update = page => page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
const status = page => page.locator('.threadNav.desktop .nativeUpdaterStatus').first();
const serverLink = url => `<a href="${url}" rel="nofollow noreferrer noopener">${url}</a>`;

test('linkification preserves filtering when generated markup would exceed its parser budget', async ({ page, owned }) => {
  const seed = 'https://seed.test/path';
  const original = `Budget needle ${serverLink(seed)}`;
  const longComment = `${original} ${'HTTP://EXAMPLE.test/a '.repeat(700)}`;
  const reply = await owned.reply(`Budget needle ${seed}`);
  const normal = await owned.reply('Normal https://seed.test/path HTTP://NORMAL.test/a');
  // The default demo board has a smaller posting limit. Substitute a synthetic
  // comment within the supported 16,000-character ceiling in its owned response
  // to exercise the real client/filter interaction without changing board policy.
  await page.route(`**${owned.url}`, async route => {
    const response = await route.fetch();
    const body = await response.text();
    if (!body.includes(original)) throw new Error('Owned filter-budget fixture was not found');
    await route.fulfill({ response, body: body.replace(original, longComment) });
  });
  await page.addInitScript(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ filter: true, linkify: false }));
    localStorage.setItem('4chan-filters', JSON.stringify([
      { type: 2, pattern: 'needle', boards: '', active: true, auto: false, hide: true },
    ]));
  });
  await page.goto(owned.url);
  const message = page.locator(`#m${reply}`);
  const post = page.locator(`#p${reply}`);
  await expect(post).toHaveClass(/post-hidden/);
  const before = await message.innerHTML();
  expect(before.length).toBeLessThan(65536);
  expect(await message.evaluate(node => node.textContent.length)).toBeLessThanOrEqual(16000);
  await message.evaluate(node => { window.budgetServerAnchor = node.querySelector('a'); });

  // Saving settings navigates that tab. Use the real settings UI in another tab
  // so the first tab receives an actual storage event and retains its DOM nodes.
  const settingsPage = await page.context().newPage();
  try {
    await settingsPage.goto(owned.url);
    const dialog = await openWatcherSettings(settingsPage);
    const navigation = dialog.getByRole('button', { name: 'Navigation', exact: true });
    if (await navigation.getAttribute('aria-expanded') === 'false') await navigation.click();
    await dialog.getByLabel('Linkify URLs', { exact: true }).check();
    await Promise.all([
      settingsPage.waitForEvent('load'),
      dialog.getByRole('button', { name: 'Save Settings', exact: true }).click(),
    ]);
  } finally { await settingsPage.close(); }
  await expect(page.locator(`#m${normal} ${generated}`)).toHaveCount(1);
  await expect(message.locator(generated)).toHaveCount(0);
  expect(await message.innerHTML()).toBe(before);
  expect(await message.evaluate(node => window.budgetServerAnchor === node.querySelector('a'))).toBe(true);
  // Force the normal cross-tab refresh path to consume the final decorated DOM,
  // so an earlier successful match cannot mask a later parser-budget failure.
  await page.evaluate(() => window.dispatchEvent(new StorageEvent('storage', { key: '4chan-filters' })));
  await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
  await expect(post).toHaveClass(/post-hidden/);
});

async function serveInitialUrlAsText(page, owned) {
  await page.route(`**${owned.url}`, async route => {
    const response = await route.fetch();
    let body = await response.text();
    const linked = serverLink('https://initial.test/path');
    if (!body.includes(linked)) throw new Error('Initial server link fixture was not found');
    body = body.replace(linked, 'https://initial.test/path');
    await route.fulfill({ response, body });
  });
}

test('desktop starts with source linkification off and the real setting enables it after save', async ({ page, owned }) => {
  await serveInitialUrlAsText(page, owned);
  await page.goto(owned.url);
  const message = page.locator(`#m${owned.id}`);
  await expect(message).toContainText('https://initial.test/path');
  await expect(message.locator('a')).toHaveCount(0);

  const dialog = await openWatcherSettings(page);
  await expect(dialog.getByLabel('Linkify URLs', { exact: true })).not.toBeChecked();
  await dialog.getByRole('button', { name: 'Close settings' }).click();
  await saveWatcherSettings(page, { linkify: true });

  await expect(message.locator(generated)).toHaveCount(1);
  await expect(message.locator(generated)).toHaveAttribute('href', '/derefer?url=https%3A%2F%2Finitial.test%2Fpath');
  await expect(message.locator(generated)).toHaveAttribute('rel', 'noreferrer ugc noopener');
});

test('mobile default, never-mobile exact value, disableAll and viewport changes use the live source option precedence', async ({ page, owned }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ linkify: false })));
  await serveInitialUrlAsText(page, owned);
  await page.goto(owned.url);
  const message = page.locator(`#m${owned.id}`);
  await expect(message.locator(generated)).toHaveCount(0);

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(message.locator(generated)).toHaveCount(1);
  let dialog = await openWatcherSettings(page);
  await expect(dialog.getByLabel('Linkify URLs', { exact: true })).toBeChecked();
  await dialog.getByRole('button', { name: 'Close settings' }).click();

  await page.evaluate(() => {
    localStorage.setItem('4chan_never_show_mobile', 'true');
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile' }));
  });
  await expect(message.locator(generated)).toHaveCount(0);
  dialog = await openWatcherSettings(page);
  await expect(dialog.getByLabel('Linkify URLs', { exact: true })).not.toBeChecked();
  await dialog.getByRole('button', { name: 'Close settings' }).click();

  await page.evaluate(() => {
    localStorage.setItem('4chan_never_show_mobile', 'TRUE');
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan_never_show_mobile' }));
  });
  await expect(message.locator(generated)).toHaveCount(1);

  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ linkify: true, disableAll: true }));
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
  });
  await expect(message.locator(generated)).toHaveCount(0);

  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ linkify: false }));
    localStorage.removeItem('4chan_never_show_mobile');
    window.dispatchEvent(new StorageEvent('storage', { key: null }));
  });
  await expect(message.locator(generated)).toHaveCount(1);
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(message.locator(generated)).toHaveCount(0);
});

test('updater raw replies, quote previews and bounded postMessage fixtures linkify while server anchors survive live disable', async ({ page, request, owned }) => {
  const quoted = await owned.reply(`>>${owned.id}\nExisting server quote`);
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ linkify: true })));
  await page.goto(owned.url);
  const quote = page.locator(`#m${quoted} .quotelink`);
  await expect(quote).toHaveAttribute('href', `/demo/post/${owned.id}`);
  await expect(page.locator(`#m${owned.id} a[href="https://initial.test/path"]`)).toHaveCount(1);
  await page.evaluate(({ id, quoted }) => {
    window.serverQuote = document.querySelector(`#m${quoted} .quotelink`);
    window.serverExternal = document.querySelector(`#m${id} a[href="https://initial.test/path"]`);
  }, { id: owned.id, quoted });

  const reply = await owned.reply('Dynamic https://reply.test/new');
  const snapshot = await (await request.get(owned.path)).json();
  const incoming = snapshot.posts.find(post => post.no === reply);
  expect(incoming).toBeTruthy();
  expect(incoming.html).toContain(serverLink('https://reply.test/new'));
  incoming.html = incoming.html.replace(serverLink('https://reply.test/new'), 'https://reply.test/new');
  await page.route(`**${owned.path}`, route => route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) }));
  await update(page);
  await expect(status(page)).toHaveText('1 new post');
  await expect(page.locator(`#m${reply} ${generated}`)).toHaveAttribute('href', '/derefer?url=https%3A%2F%2Freply.test%2Fnew');

  await page.evaluate(() => {
    const preview = document.createElement('article');
    preview.id = 'quote-preview'; preview.className = 'post preview';
    const message = document.createElement('blockquote');
    message.id = 'preview-message'; message.className = 'postMessage';
    message.textContent = 'Preview https://preview.test/new';
    preview.append(message); document.body.append(preview);

    const thread = document.querySelector('.thread');
    const uppercase = document.createElement('blockquote');
    uppercase.id = 'uppercase-message'; uppercase.className = 'postMessage';
    uppercase.textContent = 'HTTPS://UPPER.TEST/path'; thread.append(uppercase);
    const mixed = document.createElement('blockquote');
    mixed.id = 'mixed-message'; mixed.className = 'postMessage';
    mixed.textContent = 'https://lower.test/path HTTPS://UPPER.TEST/path'; thread.append(mixed);
    const bounded = document.createElement('blockquote');
    bounded.id = 'bounded-message'; bounded.className = 'postMessage';
    let branch = bounded;
    for (let depth = 0; depth < 34; depth++) { const span = document.createElement('span'); branch.append(span); branch = span; }
    branch.textContent = 'https://deep.test/path'; thread.append(bounded);
  });
  await expect(page.locator(`#preview-message ${generated}`)).toHaveAttribute('href', '/derefer?url=https%3A%2F%2Fpreview.test%2Fnew');
  await expect(page.locator('#uppercase-message a.linkified')).toHaveCount(0);
  await expect(page.locator('#mixed-message a.linkified')).toHaveCount(2);
  await expect(page.locator('#bounded-message a.linkified')).toHaveCount(0);
  await expect(page.locator('#bounded-message')).toContainText('https://deep.test/path');

  await page.evaluate(() => {
    localStorage.setItem('4chan-settings', JSON.stringify({ linkify: true, disableAll: true }));
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
  });
  await expect(page.locator(generated)).toHaveCount(0);
  expect(await page.evaluate(({ id, quoted }) => {
    const currentQuote = document.querySelector(`#m${quoted} .quotelink`);
    const currentExternal = document.querySelector(`#m${id} a[href="https://initial.test/path"]`);
    return {
      quote: currentQuote === window.serverQuote && currentQuote?.isConnected && currentQuote.getAttribute('href'),
      external: currentExternal === window.serverExternal && currentExternal?.isConnected && currentExternal.getAttribute('href'),
    };
  }, { id: owned.id, quoted })).toEqual({ quote: `/demo/post/${owned.id}`, external: 'https://initial.test/path' });
});

test('persisted mixed-case URLs keep server anchors while the browser links uppercase text in initial HTML and updater replies', async ({ page, request }) => {
  const password = 'owned-linkification-mixed-password';
  const lower = 'https://lower.test/path';
  const upper = 'HTTPS://UPPER.TEST/Path?Q=One';
  const comment = `Existing ${lower} then ${upper}`;
  const write = form => request.post('/demo/post', {
    headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password },
  });
  const created = await write({ resto: '0', sub: 'Persisted mixed linkification', com: comment });
  expect(created.status()).toBe(303);
  const id = created.headers().location.match(/thread\/(\d+)/)[1];
  const url = `/demo/thread/${id}`;
  const jsonUrl = `/demo/thread/${id}.json`;
  try {
    const htmlBefore = await (await request.get(url)).text();
    const jsonBefore = await (await request.get(jsonUrl)).json();
    const storedBefore = jsonBefore.posts.find(post => String(post.no) === id)?.com;
    expect(storedBefore).toContain(serverLink(lower));
    expect(storedBefore).toContain(upper);
    expect(storedBefore).not.toContain(`/derefer?url=${encodeURIComponent(upper)}`);

    await page.goto(url);
    const initial = page.locator(`#m${id}`);
    const initialLower = initial.locator(`a[href="${lower}"]`);
    await expect(initialLower).toHaveCount(1);
    await expect(initial).toContainText(upper);
    await expect(initial.locator(generated)).toHaveCount(0);
    await page.evaluate(({ id, lower }) => {
      window.persistedLowerAnchor = document.querySelector(`#m${id} a[href="${lower}"]`);
    }, { id, lower });

    await page.evaluate(() => {
      localStorage.setItem('4chan-settings', JSON.stringify({ linkify: true }));
      window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
    });
    const initialUpper = initial.locator(generated);
    await expect(initialUpper).toHaveCount(1);
    await expect(initialUpper).toHaveText(upper);
    await expect(initialUpper).toHaveAttribute('href', `/derefer?url=${encodeURIComponent(upper)}`);
    expect(await page.evaluate(({ id, lower }) => {
      const current = document.querySelector(`#m${id} a[href="${lower}"]`);
      return current === window.persistedLowerAnchor && current?.isConnected && current.getAttribute('href');
    }, { id, lower })).toBe(lower);

    const htmlAfter = await (await request.get(url)).text();
    const jsonAfter = await (await request.get(jsonUrl)).json();
    expect(htmlAfter).toBe(htmlBefore);
    expect(jsonAfter.posts.find(post => String(post.no) === id)?.com).toBe(storedBefore);

    const posted = await write({ resto: id, com: `Reply ${lower} and ${upper}` });
    expect(posted.status()).toBe(303);
    const reply = posted.headers().location.match(/#p(\d+)/)[1];
    await update(page);
    await expect(status(page)).toHaveText('1 new post');
    const replyMessage = page.locator(`#m${reply}`);
    const replyLower = replyMessage.locator(`a[href="${lower}"]`);
    const replyUpper = replyMessage.locator(generated);
    await expect(replyLower).toHaveCount(1);
    await expect(replyUpper).toHaveCount(1);
    await expect(replyUpper).toHaveText(upper);
    await expect(replyUpper).toHaveAttribute('href', `/derefer?url=${encodeURIComponent(upper)}`);

    const persisted = await (await request.get(jsonUrl)).json();
    const storedReply = persisted.posts.find(post => String(post.no) === reply)?.com;
    expect(storedReply).toContain(serverLink(lower));
    expect(storedReply).toContain(upper);
    expect(storedReply).not.toContain(`/derefer?url=${encodeURIComponent(upper)}`);
  } finally {
    await request.post('/demo/delete', {
      headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
    });
  }
});

test('mobile default remains usable when settings storage reads are unavailable', async ({ page, owned }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    Object.defineProperty(Storage.prototype, 'getItem', {
      configurable: true,
      value() { throw new DOMException('Unavailable', 'SecurityError'); },
    });
  });
  await serveInitialUrlAsText(page, owned);
  await page.goto(owned.url);
  await expect(page.locator(`#m${owned.id} ${generated}`)).toHaveCount(1);
  const dialog = await openWatcherSettings(page);
  await expect(dialog.getByLabel('Linkify URLs', { exact: true })).toBeChecked();
});

test('catalog pages do not mount board linkification or quote-preview observation', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ linkify: true })));
  await page.goto('/demo/catalog');
  await page.evaluate(() => {
    const preview = document.createElement('article');
    preview.id = 'quote-preview';
    const message = document.createElement('blockquote');
    message.className = 'postMessage'; message.textContent = 'https://catalog.test/plain';
    preview.append(message); document.body.append(preview);
  });
  await page.waitForTimeout(50);
  await expect(page.locator('#quote-preview a.linkified')).toHaveCount(0);
  await expect(page.locator('#quote-preview')).toContainText('https://catalog.test/plain');
});
