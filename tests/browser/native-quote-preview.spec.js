import { test as base, expect } from '@playwright/test';
import { openWatcherSettings } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';
const escaped = '<script>window.quotePreviewInjected = true</script>\n<img src="/__quote-preview-escaped.png" onerror="window.quotePreviewInjected = true">';
const localComment = `Owned local preview target\n>green\n${escaped}`;
const trailingLines = Array.from({ length: 28 }, (_, index) => `Persisted trailing line ${index + 1}`).join('\n');
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];

const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-quote-preview-password';
    const threads = [];
    const write = async (board, resto, com) => {
      const response = await request.post(`/${board}/post`, {
        headers: { Origin: origin }, maxRedirects: 0,
        form: { resto, com, password, ...(resto === '0' ? { sub: 'Owned quote preview' } : {}) },
      });
      expect(response.status(), 'Persisted preview fixture must be accepted').toBe(303);
      const location = response.headers().location;
      const match = location?.match(/\/thread\/(\d+)#p(\d+)$/);
      expect(match, 'Posting must return its canonical thread and post IDs').not.toBeNull();
      return match[2];
    };
    const createThread = async (board = 'demo', comment = localComment) => {
      const id = await write(board, '0', comment);
      const thread = {
        board, id, url: `/${board}/thread/${id}`, previewPath: `/_watch/${board}/post/${id}`,
        updatesPath: `/_watch/${board}/thread/${id}/posts`,
        reply: com => write(board, id, com),
      };
      threads.push(thread);
      return thread;
    };
    try {
      await use({ ...await createThread(), createThread });
    } finally {
      for (const { board, id } of threads.reverse()) {
        const response = await request.post(`/${board}/delete`, {
          headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
        });
        expect(response.status(), 'Owned preview thread cleanup must succeed').toBe(303);
      }
    }
  },
});

async function initialize(page, url, settings = { quotePreview: true }) {
  await page.addInitScript(settings => {
    // The settings UI reloads after saving. Preserve that real saved value.
    if (localStorage.getItem('4chan-settings') === null) {
      localStorage.setItem('4chan-settings', JSON.stringify(settings));
    }
  }, settings);
  await page.goto(url);
  await expect(page.locator('#settingsWindowLink:visible, #settingsWindowLinkMobile:visible')).toBeVisible();
}

function network(page) {
  const requests = [];
  page.on('request', request => {
    if (['fetch', 'xhr'].includes(request.resourceType())) requests.push(request);
  });
  return requests;
}

function quote(page, reply, target) {
  return page.locator(`#m${reply} a.quotelink[href="/${target.board}/post/${target.id}"]`);
}

async function leaveQuote(page) {
  await page.mouse.move(1, 1);
  await expect(page.locator('#quote-preview')).toHaveCount(0);
}

async function hoverOffscreenLocal(page, owned, link) {
  // Real persisted lines below this link provide scroll room; no DOM fixture is
  // substituted for the post that the preview must inspect.
  await link.evaluate(node => node.scrollIntoView({ block: 'start' }));
  await page.evaluate(() => scrollBy(0, -40));
  expect(await page.locator(`#p${owned.id}`).evaluate(node => {
    const rect = node.getBoundingClientRect();
    return rect.top > 0 && rect.bottom < document.documentElement.clientHeight;
  })).toBe(false);
  await link.hover();
}

async function expectPreview(page, target, text) {
  const preview = page.locator('#quote-preview');
  await expect(preview).toBeVisible();
  await expect(preview.locator('.postMessage')).toHaveCount(1);
  await expect(preview.locator('.postMessage')).toContainText(text);
  await expect(preview.locator('.postNum')).toHaveAttribute('href', `${target.url}#p${target.id}`);
  await expect(preview.locator('[id], form, input, button, details, script, iframe, object, embed')).toHaveCount(0);
  expect(await page.locator('[id]').evaluateAll(nodes => {
    const ids = nodes.map(node => node.id);
    return new Set(ids).size === ids.length;
  })).toBe(true);
  return preview;
}

async function savePreviewSettings(page, values) {
  const dialog = await openWatcherSettings(page);
  const navigation = dialog.getByRole('button', { name: 'Navigation', exact: true });
  if (await navigation.getAttribute('aria-expanded') === 'false') await navigation.click();
  for (const [key, value] of Object.entries(values)) {
    await dialog.locator(`.menuOption[data-option="${key}"]`).setChecked(value);
  }
  await Promise.all([
    page.waitForEvent('load'),
    dialog.getByRole('button', { name: 'Save Settings', exact: true }).click(),
  ]);
}

test.describe('unmodified persisted quote previews', () => {
  test('Quote preview defaults on and its real saved option survives navigation', async ({ page, owned }) => {
    const reply = await owned.reply(`>>${owned.id}\n${trailingLines}`);
    await page.setViewportSize({ width: 1280, height: 400 });
    await page.goto(owned.url);
    const dialog = await openWatcherSettings(page);
    const navigation = dialog.getByRole('button', { name: 'Navigation', exact: true });
    if (await navigation.getAttribute('aria-expanded') === 'false') await navigation.click();
    await expect(dialog.getByLabel('Quote preview', { exact: true })).toBeChecked();
    await dialog.getByRole('button', { name: 'Close settings', exact: true }).click();

    await savePreviewSettings(page, { quotePreview: false });
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).quotePreview)).toBe(false);
    const requests = network(page);
    await hoverOffscreenLocal(page, owned, quote(page, reply, owned));
    await page.waitForTimeout(150);
    await expect(page.locator('#quote-preview')).toHaveCount(0);
    expect(requests).toEqual([]);

    await savePreviewSettings(page, { quotePreview: true });
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).quotePreview)).toBe(true);
    await hoverOffscreenLocal(page, owned, quote(page, reply, owned));
    await expectPreview(page, owned, 'Owned local preview target');
  });

  test('a fully visible local target uses highlighting without a popup or fetch', async ({ page, owned }) => {
    const reply = await owned.reply(`>>${owned.id}\nVisible target quote`);
    await initialize(page, owned.url);
    const link = quote(page, reply, owned), target = page.locator(`#p${owned.id}`);
    expect(await target.evaluate(node => {
      const rect = node.getBoundingClientRect();
      return rect.top > 0 && rect.bottom < document.documentElement.clientHeight;
    })).toBe(true);
    const requests = network(page);
    const originalClass = await target.getAttribute('class');
    await link.hover();
    await expect(target).toHaveClass(/\bhighlight(?:-anti)?\b/);
    await expect(page.locator('#quote-preview')).toHaveCount(0);
    expect(requests).toEqual([]);
    await leaveQuote(page);
    await expect(target).toHaveAttribute('class', originalClass);
  });

  test('an offscreen live post previews escaped text without fetching, moving nodes, or duplicating forms', async ({ page, owned }) => {
    const reply = await owned.reply(`>>${owned.id}\n${trailingLines}`);
    await page.setViewportSize({ width: 1280, height: 400 });
    await initialize(page, owned.url);
    const requests = network(page);
    const original = await page.locator(`#p${owned.id}`).innerHTML();
    await page.locator(`#p${owned.id}`).evaluate(node => { window.previewSourcePost = node; });
    await hoverOffscreenLocal(page, owned, quote(page, reply, owned));
    const preview = await expectPreview(page, owned, 'Owned local preview target');
    await expect(preview.locator('.postMessage')).toContainText('<script>window.quotePreviewInjected = true</script>');
    await expect(preview.locator('.postMessage img')).toHaveCount(0);
    expect(requests).toEqual([]);
    expect(await page.evaluate(() => window.quotePreviewInjected)).toBeUndefined();
    expect(await page.locator(`#p${owned.id}`).evaluate(node => node === window.previewSourcePost)).toBe(true);
    expect(await page.locator(`#p${owned.id}`).innerHTML()).toBe(original);
    await leaveQuote(page);
  });

  test('a reply hidden through its real post menu can be previewed without unhiding it or fetching', async ({ page, owned }) => {
    const hidden = await owned.reply('Owned hidden reply preview target');
    const reply = await owned.reply(`>>${hidden}\nQuote to hidden reply`);
    const target = { ...owned, id: hidden };
    await initialize(page, owned.url);
    await page.getByRole('button', { name: `Post menu for post ${hidden}`, exact: true }).click();
    await page.getByRole('menuitem', { name: 'Hide post', exact: true }).click();
    await expect(page.locator(`#pc${hidden}`)).toHaveClass(/post-hidden/);
    await expect(page.locator(`#m${hidden}`)).toBeHidden();
    const requests = network(page);
    await quote(page, reply, target).hover();
    await expectPreview(page, target, 'Owned hidden reply preview target');
    await expect(page.locator(`#pc${hidden}`)).toHaveClass(/post-hidden/);
    await expect(page.locator(`#m${hidden}`)).toBeHidden();
    expect(requests).toEqual([]);
    await leaveQuote(page);
  });

  for (const board of ['demo', 'test']) {
    test(`a persisted ${board === 'demo' ? 'cross-thread' : 'cross-board'} quote fetches only its owned post endpoint`, async ({ page, request, owned }) => {
      const remote = await owned.createThread(board, `Remote target /${board}/\n${escaped}`);
      await remote.reply('Another post that must stay out of the preview');
      const reply = await owned.reply(`${board === owned.board ? `>>${remote.id}` : `>>>/${board}/${remote.id}`}\nRemote quote`);
      await initialize(page, owned.url);
      await expect(page.locator(`#p${remote.id}`)).toHaveCount(0);
      const requests = network(page);
      const responsePromise = page.waitForResponse(response => new URL(response.url()).pathname === remote.previewPath);
      await quote(page, reply, remote).hover();
      const response = await responsePromise;
      expect(response.status()).toBe(200);
      expect(response.headers()['content-type']).toContain('application/json');
      expect(response.headers().etag).toMatch(/^"[0-9a-f]{64}"$/);
      const body = await response.body(), snapshot = JSON.parse(body.toString('utf8'));
      expect(body.byteLength).toBeLessThanOrEqual(262144);
      expect(snapshot).toEqual({
        version: 1, board, thread: remote.id,
        post: { no: remote.id, file_deleted: false, html: expect.any(String) },
      });
      const preview = await expectPreview(page, remote, `Remote target /${board}/`);
      await expect(preview).not.toContainText('Another post that must stay out of the preview');
      await expect(preview.locator('.postMessage')).toContainText('<script>window.quotePreviewInjected = true</script>');
      expect(await page.evaluate(() => window.quotePreviewInjected)).toBeUndefined();
      expect(requests.map(value => value.url())).toEqual([`${origin}${remote.previewPath}`]);
      const conditional = await request.get(remote.previewPath, { headers: { 'If-None-Match': response.headers().etag } });
      expect(conditional.status()).toBe(304);
      expect(await conditional.text()).toBe('');
      await leaveQuote(page);
      await expect(quote(page, reply, remote)).toHaveAttribute('href', `/${board}/post/${remote.id}`);
    });
  }

  test('an omitted index reply is fetched while the index and omission link remain intact', async ({ page, owned }) => {
    const omitted = await owned.reply('Persisted omitted preview target');
    for (let index = 0; index < 3; index++) await owned.reply(`Newer visible reply ${index + 1}`);
    const reply = await owned.reply(`>>${omitted}\nQuote to omitted reply`);
    const target = { ...owned, id: omitted, previewPath: `/_watch/demo/post/${omitted}` };
    await initialize(page, '/demo/');
    await expect(page.locator(`#t${owned.id} .omitted`)).toContainText('posts omitted');
    await expect(page.locator(`#p${omitted}`)).toHaveCount(0);
    const requests = network(page);
    await quote(page, reply, target).hover();
    await expectPreview(page, target, 'Persisted omitted preview target');
    expect(requests.map(value => value.url())).toEqual([`${origin}${target.previewPath}`]);
    await expect(page).toHaveURL(`${origin}/demo/`);
    await expect(page.locator(`#t${owned.id} .omitted a`)).toHaveAttribute('href', owned.url);
  });

  test('the real updater inserts working local and remote quote links into the existing document', async ({ page, owned }) => {
    const remote = await owned.createThread('test', 'Updater remote preview target');
    await page.setViewportSize({ width: 1280, height: 400 });
    await initialize(page, owned.url);
    await page.evaluate(() => { window.quotePreviewDocument = document; });
    const reply = await owned.reply(`>>${owned.id}\n>>>/test/${remote.id}\n${trailingLines}`);
    await page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
    await expect(page.locator('.threadNav.desktop .nativeUpdaterStatus').first()).toHaveText('1 new post');
    expect(await page.evaluate(() => document === window.quotePreviewDocument)).toBe(true);
    const requests = network(page);
    await hoverOffscreenLocal(page, owned, quote(page, reply, owned));
    await expectPreview(page, owned, 'Owned local preview target');
    expect(requests).toEqual([]);
    await leaveQuote(page);
    await quote(page, reply, remote).hover();
    await expectPreview(page, remote, 'Updater remote preview target');
    expect(requests.map(value => value.url())).toEqual([`${origin}${remote.previewPath}`]);
    await expect(page.locator(`#p${reply}`)).toHaveCount(1);
  });

  test('desktop focus and Enter retain canonical navigation without opening a preview', async ({ page, owned }) => {
    const remote = await owned.createThread('test', 'Keyboard destination');
    const reply = await owned.reply(`>>>/test/${remote.id}\nKeyboard quote`);
    await initialize(page, owned.url);
    const link = quote(page, reply, remote), requests = network(page);
    await link.focus();
    await expect(link).toBeFocused();
    await page.waitForTimeout(150);
    await expect(page.locator('#quote-preview')).toHaveCount(0);
    expect(requests).toEqual([]);
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(`${origin}${remote.url}#p${remote.id}`);
    await expect(page.locator(`#m${remote.id}`)).toHaveText('Keyboard destination');
  });

  test('desktop click and no-JavaScript click follow the same persisted quote destination', async ({ page, browser, owned }) => {
    const remote = await owned.createThread('test', 'Canonical click destination');
    const reply = await owned.reply(`>>>/test/${remote.id}\nCanonical click quote`);
    await initialize(page, owned.url);
    await quote(page, reply, remote).click();
    await expect(page).toHaveURL(`${origin}${remote.url}#p${remote.id}`);
    const context = await browser.newContext({ javaScriptEnabled: false });
    try {
      const plain = await context.newPage();
      await plain.goto(`${origin}${owned.url}`);
      await expect(quote(plain, reply, remote)).toHaveAttribute('href', `/test/post/${remote.id}`);
      await quote(plain, reply, remote).click();
      await expect(plain).toHaveURL(`${origin}${remote.url}#p${remote.id}`);
      await expect(plain.locator(`#m${remote.id}`)).toHaveText('Canonical click destination');
      await expect(plain.locator('#quote-preview')).toHaveCount(0);
    } finally { await context.close(); }
  });

  test('real settings saves in another tab remove active previews and honor disableAll without reloading this tab', async ({ page, context, owned }) => {
    const reply = await owned.reply(`>>${owned.id}\n${trailingLines}`);
    await page.setViewportSize({ width: 1280, height: 400 });
    await initialize(page, owned.url);
    const other = await context.newPage();
    try {
      await other.goto(owned.url);
      await page.evaluate(() => { window.quotePreviewDocument = document; });
      const requests = network(page), link = quote(page, reply, owned);
      await hoverOffscreenLocal(page, owned, link);
      await expectPreview(page, owned, 'Owned local preview target');
      await savePreviewSettings(other, { quotePreview: false });
      await expect(page.locator('#quote-preview')).toHaveCount(0);
      await leaveQuote(page); await hoverOffscreenLocal(page, owned, link);
      await page.waitForTimeout(150);
      await expect(page.locator('#quote-preview')).toHaveCount(0);
      await savePreviewSettings(other, { quotePreview: true });
      await leaveQuote(page); await hoverOffscreenLocal(page, owned, link);
      await expectPreview(page, owned, 'Owned local preview target');
      await savePreviewSettings(other, { disableAll: true });
      await expect(page.locator('#quote-preview')).toHaveCount(0);
      await savePreviewSettings(other, { disableAll: false });
      await leaveQuote(page); await hoverOffscreenLocal(page, owned, link);
      await expectPreview(page, owned, 'Owned local preview target');
      expect(await page.evaluate(() => document === window.quotePreviewDocument)).toBe(true);
      expect(requests).toEqual([]);
    } finally { await other.close(); }
  });

  test('a mobile-device tap previews the original quote and the adjacent # link keeps navigation', async ({ browser, owned }, testInfo) => {
    const remote = await owned.createThread('test', 'Mobile remote preview target');
    const reply = await owned.reply(`>>>/test/${remote.id}\n${trailingLines}`);
    const context = await browser.newContext({
      viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true,
      userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36',
    });
    const page = await context.newPage();
    try {
      await initialize(page, `${origin}${owned.url}`);
      const link = quote(page, reply, remote), navigation = page.locator(`#m${reply} a.quoteLink`);
      await expect(navigation).toHaveCount(1);
      await expect(navigation).toHaveText(' #');
      await expect(navigation).toHaveAttribute('href', `/test/post/${remote.id}`);
      await link.evaluate(node => node.scrollIntoView({ block: 'start' }));
      await page.evaluate(() => scrollBy(0, -40));
      await page.evaluate(() => {
        const identify = node => node?.nodeType === 1
          ? { tag: node.tagName, id: node.id, class: node.className } : null;
        const events = window.mobileQuoteEvents = [];
        const record = data => { if (events.length < 96) events.push({ at: performance.now(), ...data }); };
        for (const type of ['pointerdown', 'pointerup', 'touchstart', 'touchend', 'mouseover', 'mouseout', 'click']) {
          document.addEventListener(type, event => record({
            type, target: identify(event.target), related: identify(event.relatedTarget),
            x: event.clientX, y: event.clientY, touch: event.sourceCapabilities?.firesTouchEvents,
            trusted: event.isTrusted, pointerType: event.pointerType,
            preview: !!document.getElementById('quote-preview'),
          }), true);
        }
        new MutationObserver(records => {
          for (const item of records) {
            for (const node of item.addedNodes) if (node.id === 'quote-preview') record({ type: 'preview-added' });
            for (const node of item.removedNodes) if (node.id === 'quote-preview') record({ type: 'preview-removed' });
          }
        }).observe(document.body, { childList: true });
      });
      const requests = network(page);
      await link.tap();
      const preview = await expectPreview(page, remote, 'Mobile remote preview target');
      await expect(page).toHaveURL(`${origin}${owned.url}`);
      expect(requests.map(value => value.url())).toEqual([`${origin}${remote.previewPath}`]);
      const bounds = await preview.boundingBox(), linkBounds = await link.boundingBox();
      expect(bounds.y).toBeGreaterThanOrEqual(linkBounds.y + linkBounds.height - 1);
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(391);
      await navigation.tap();
      await expect(page).toHaveURL(`${origin}${remote.url}#p${remote.id}`);
      await expect(page.locator(`#m${remote.id}`)).toHaveText('Mobile remote preview target');
    } catch (error) {
      const events = await page.evaluate(() => window.mobileQuoteEvents ?? []).catch(() => []);
      await testInfo.attach('mobile-quote-events', { body: JSON.stringify(events, null, 2), contentType: 'application/json' });
      console.error('Mobile quote event sequence:', JSON.stringify(events));
      throw error;
    } finally { await context.close(); }
  });

  for (const theme of themes) {
    test(`offscreen preview stays within the desktop viewport in ${theme}`, async ({ page, context, owned }) => {
      const reply = await owned.reply(`>>${owned.id}\n${trailingLines}`);
      await context.addCookies([{ name: 'board-theme-ws', value: theme, url: origin, httpOnly: true, sameSite: 'Lax' }]);
      await page.setViewportSize({ width: 1000, height: 440 });
      await initialize(page, owned.url);
      await hoverOffscreenLocal(page, owned, quote(page, reply, owned));
      const preview = await expectPreview(page, owned, 'Owned local preview target');
      const bounds = await preview.boundingBox();
      expect(bounds.width).toBeGreaterThan(0);
      expect(bounds.height).toBeGreaterThan(0);
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.y).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(1001);
      expect(bounds.y + bounds.height).toBeLessThanOrEqual(441);
      expect(await preview.evaluate(node => getComputedStyle(node).backgroundColor)).not.toBe('rgba(0, 0, 0, 0)');
      const sourceStyle = await page.locator(`#p${reply}`).evaluate(node => {
        const style = getComputedStyle(node);
        return { background: style.backgroundColor, color: style.color, font: style.fontFamily };
      });
      expect(await preview.evaluate(node => {
        const style = getComputedStyle(node);
        return { background: style.backgroundColor, color: style.color, font: style.fontFamily };
      })).toEqual(sourceStyle);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      await leaveQuote(page);
    });
  }
});

test.describe('explicitly augmented DOM fixtures', () => {
  for (const defect of ['inert resource markup', 'excessive nesting']) {
    test(`local ${defect} cannot load resources or move the source, and removal retains a healthy preview`, async ({ page, owned }) => {
      const reply = await owned.reply(`>>${owned.id}\n${trailingLines}`);
      await page.setViewportSize({ width: 1280, height: 400 });
      await observePreviewTransport(page);
      await initialize(page, owned.url);
      const resources = [];
      page.on('request', request => {
        if (new URL(request.url()).searchParams.has('native-quote-probe')) resources.push(request.url());
      });
      const control = '/static/themes/fade.png?native-quote-probe=local-control';
      expect(await page.evaluate(src => new Promise(resolve => {
        const image = new Image();
        image.onload = () => resolve(true); image.onerror = () => resolve(false); image.src = src;
      }), control)).toBe(true);
      expect(resources).toEqual([`${origin}${control}`]);
      const requests = network(page);
      await page.locator(`#m${owned.id}`).evaluate((message, defect) => {
        let root;
        if (defect === 'inert resource markup') {
          root = document.createElement('template');
          // Create the image in the inert template document, so the fixture does
          // not itself load the resource that preview construction must reject.
          const image = root.content.ownerDocument.createElement('img');
          image.setAttribute('src', '/static/themes/fade.png?native-quote-probe=local-payload');
          root.content.append(image);
        } else {
          root = document.createElement('span');
          let nested = root;
          for (let depth = 0; depth < 40; depth++) {
            const child = document.createElement('span'); nested.append(child); nested = child;
          }
          nested.textContent = 'Over-depth local input';
        }
        message.append(root); window.ownedHostileLocalNode = root;
      }, defect);
      await hoverOffscreenLocal(page, owned, quote(page, reply, owned));
      if (defect === 'inert resource markup') {
        const preview = await expectPreview(page, owned, 'Owned local preview target');
        await expect(preview.locator('template, img')).toHaveCount(0);
      } else {
        await page.waitForTimeout(150);
        await expect(page.locator('#quote-preview .postMessage')).toHaveCount(0);
        expect(await page.evaluate(() => window.ownedPreviewCommits)).toEqual([]);
      }
      expect(await page.evaluate(() => window.ownedHostileLocalNode.isConnected)).toBe(true);
      expect(resources).toEqual([`${origin}${control}`]);
      expect(requests).toEqual([]);
      await leaveQuote(page);
      await page.evaluate(() => window.ownedHostileLocalNode.remove());
      await hoverOffscreenLocal(page, owned, quote(page, reply, owned));
      await expectPreview(page, owned, 'Owned local preview target');
      expect(resources).toEqual([`${origin}${control}`]);
      expect(requests).toEqual([]);
    });
  }

  test('a canonical thread fragment quote keeps its href and resolves through the single-post endpoint', async ({ page, owned }) => {
    const remote = await owned.createThread('test', 'Canonical thread-fragment target');
    const reply = await owned.reply(`>>>/test/${remote.id}\nThread-fragment quote`);
    await initialize(page, owned.url);
    const link = quote(page, reply, remote);
    const canonical = `${remote.url}#p${remote.id}`;
    // Persisted comment rendering emits /post/id. This one explicit href change
    // covers the other supported quote route without relabeling it as server HTML.
    await link.evaluate((node, href) => { node.href = href; }, canonical);
    const canonicalLink = page.locator(`#m${reply} a.quotelink`), requests = network(page);
    await canonicalLink.hover();
    await expectPreview(page, remote, 'Canonical thread-fragment target');
    expect(requests.map(value => value.url())).toEqual([`${origin}${remote.previewPath}`]);
    await expect(canonicalLink).toHaveAttribute('href', canonical);
    await canonicalLink.click();
    await expect(page).toHaveURL(`${origin}${canonical}`);
  });

  test('catalog pages do not mount quote preview behavior even when quote-shaped DOM is added', async ({ page, owned }) => {
    const reply = await owned.reply(`>>${owned.id}\n${trailingLines}`);
    await initialize(page, '/demo/catalog');
    await page.evaluate(id => {
      const container = document.createElement('article');
      container.className = 'postContainer';
      const message = document.createElement('blockquote');
      message.className = 'postMessage';
      const link = document.createElement('a');
      link.id = 'owned-catalog-quote'; link.className = 'quotelink';
      link.href = `/demo/post/${id}`; link.textContent = `>>${id}`;
      message.append(link); container.append(message);
      document.querySelector('.catalog').append(container);
    }, owned.id);
    const requests = network(page);
    await page.locator('#owned-catalog-quote').hover();
    await page.waitForTimeout(150);
    await expect(page.locator('#quote-preview')).toHaveCount(0);
    expect(requests).toEqual([]);
    // The same context can still mount the feature on a real board page.
    await page.setViewportSize({ width: 1280, height: 400 });
    await page.goto(owned.url);
    await hoverOffscreenLocal(page, owned, quote(page, reply, owned));
    await expectPreview(page, owned, 'Owned local preview target');
  });
});

async function observePreviewTransport(page) {
  await page.addInitScript(() => {
    const originalFetch = window.fetch;
    window.ownedPreviewFetches = [];
    window.fetch = function (input, options) {
      const url = new URL(input instanceof Request ? input.url : String(input), location.href);
      if (/^\/_watch\/[a-z0-9]+\/post\/[1-9][0-9]*$/.test(url.pathname)) {
        const record = { path: url.pathname, aborted: options?.signal?.aborted === true };
        window.ownedPreviewFetches.push(record);
        options?.signal?.addEventListener('abort', () => { record.aborted = true; }, { once: true });
      }
      return Reflect.apply(originalFetch, this, [input, options]);
    };
    const OriginalWorker = window.Worker;
    window.ownedPreviewWorkers = { started: 0, terminated: 0 };
    window.Worker = class extends OriginalWorker {
      constructor(...args) {
        super(...args);
        window.ownedPreviewWorkers.started++;
        this.ownedTerminated = false;
      }
      terminate() {
        if (!this.ownedTerminated) window.ownedPreviewWorkers.terminated++;
        this.ownedTerminated = true;
        return super.terminate();
      }
    };
    window.ownedPreviewCommits = [];
    new MutationObserver(() => {
      const message = document.querySelector('#quote-preview .postMessage');
      if (message) window.ownedPreviewCommits.push(message.textContent);
    }).observe(document, { childList: true, subtree: true });
  });
}

async function holdNextResponse(page, path) {
  let enter, release, complete, active = false;
  const entered = new Promise(resolve => { enter = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  const completed = new Promise(resolve => { complete = resolve; });
  const pattern = `**${path}`;
  const handler = async route => {
    active = true;
    try {
      // Hold the genuine server body and headers. Only response timing changes.
      const response = await route.fetch();
      enter();
      await gate;
      await route.fulfill({ response }).catch(() => {});
    } finally { complete(); }
  };
  await page.route(pattern, handler, { times: 1 });
  return {
    entered,
    async release() {
      release();
      await page.unroute(pattern, handler);
      if (active) await completed;
    },
  };
}

test.describe('held responses from persisted posts', () => {
  test('a real cross-tab filter hides the source quote and cancels its pending preview before the response is released', async ({ page, context, owned }) => {
    const remote = await owned.createThread('test', 'Target behind a filtered quote');
    const reply = await owned.reply(`>>>/test/${remote.id}\nHide preview source needle`);
    await observePreviewTransport(page);
    await initialize(page, owned.url, { quotePreview: true, filter: true });
    const other = await context.newPage();
    await other.goto(owned.url);
    const held = await holdNextResponse(page, remote.previewPath);
    try {
      await expect(page.locator(`#m${reply}`)).toBeVisible();
      await quote(page, reply, remote).hover();
      await held.entered;
      await expect.poll(() => page.evaluate(() => window.ownedPreviewFetches.at(-1)?.aborted)).toBe(false);
      const rules = [{ type: 2, pattern: 'Hide preview source needle', boards: 'demo', active: true, auto: false, hide: true }];
      // The browser delivers the real storage event to the first tab; the
      // production filter matcher and renderer decide which post to hide.
      await other.evaluate(rules => localStorage.setItem('4chan-filters', JSON.stringify(rules)), rules);
      await expect(page.locator(`#p${reply}`)).toHaveClass(/post-hidden/);
      await expect(page.locator(`#m${reply}`)).toBeHidden();
      await expect(page.getByRole('button', { name: `View filtered post ${reply}`, exact: true })).toBeVisible();
      await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
      await expect.poll(() => page.evaluate(() => window.ownedPreviewFetches.at(-1)?.aborted)).toBe(true);
      await held.release();
      await expect(page.locator('#quote-preview')).toHaveCount(0);
      expect(await page.evaluate(() => window.ownedPreviewCommits)).toEqual([]);

      await other.evaluate(() => localStorage.setItem('4chan-filters', '[]'));
      await expect(page.locator(`#p${reply}`)).not.toHaveClass(/post-hidden/);
      await expect(page.locator(`#m${reply}`)).toBeVisible();
      await leaveQuote(page); await page.waitForTimeout(350);
      await quote(page, reply, remote).hover();
      await expectPreview(page, remote, 'Target behind a filtered quote');
    } finally {
      await held.release();
      await other.close();
    }
  });

  for (const cancellation of ['pointer exit', 'quotePreview', 'disableAll']) {
    test(`${cancellation} aborts the owned request and its released late response cannot reopen a preview`, async ({ page, context, owned }) => {
      const remote = await owned.createThread('test', 'Held persisted preview target');
      const reply = await owned.reply(`>>>/test/${remote.id}\nHeld response quote`);
      // Observe AbortSignal without altering requests, response bodies or results.
      await observePreviewTransport(page);
      await initialize(page, owned.url);
      const other = cancellation === 'pointer exit' ? null : await context.newPage();
      if (other) await other.goto(owned.url);
      const held = await holdNextResponse(page, remote.previewPath);
      try {
        await quote(page, reply, remote).hover();
        await held.entered;
        await expect.poll(() => page.evaluate(path => window.ownedPreviewFetches.some(item => item.path === path && !item.aborted), remote.previewPath)).toBe(true);
        if (other) await savePreviewSettings(other, { [cancellation]: cancellation === 'disableAll' });
        else await leaveQuote(page);
        await expect.poll(() => page.evaluate(path => window.ownedPreviewFetches.some(item => item.path === path && item.aborted), remote.previewPath)).toBe(true);
        await held.release();
        await expect(page.locator('#quote-preview')).toHaveCount(0);

        if (other) await savePreviewSettings(other, { quotePreview: true, disableAll: false });
        // The transport has a 300 ms global cooldown. A later hover must retry
        // the same real target in this document after cancellation has settled.
        await leaveQuote(page);
        await page.waitForTimeout(350);
        await quote(page, reply, remote).hover();
        await expectPreview(page, remote, 'Held persisted preview target');
      } finally {
        await held.release();
        await other?.close();
      }
    });
  }
});

const rejectedMarker = 'Rejected preview payload';
const imageProbe = '/static/themes/fade.png?native-quote-probe=payload';

test.describe('response-substituted adversarial fixtures', () => {
  for (const defect of ['resource element', 'post identity', 'UTF-8 byte ceiling', 'tree depth', 'tree nodes']) {
    test(`${defect} rejects the complete response before preview resources load and permits a healthy retry`, async ({ page, request, owned }) => {
      const remote = await owned.createThread('test', 'Healthy persisted recovery target');
      const reply = await owned.reply(`>>>/test/${remote.id}\nAdversarial response quote`);
      const response = await request.get(remote.previewPath);
      expect(response.status()).toBe(200);
      const snapshot = await response.json();
      expect(snapshot.post.html).toContain('</blockquote>');
      let injected = rejectedMarker;
      if (defect === 'resource element') injected += `<img src="${imageProbe}" onerror="window.quotePreviewInjected = true">`;
      if (defect === 'post identity') snapshot.post.no = owned.id;
      if (defect === 'UTF-8 byte ceiling') injected += '\u6f22'.repeat(90000);
      if (defect === 'tree depth') injected += `${'<span>'.repeat(40)}nested${'</span>'.repeat(40)}`;
      if (defect === 'tree nodes') injected += 'x<wbr>'.repeat(17000);
      snapshot.post.html = snapshot.post.html.replace('</blockquote>', `${injected}</blockquote>`);
      const body = JSON.stringify(snapshot);
      if (defect === 'UTF-8 byte ceiling') {
        expect(body.length).toBeLessThan(262144);
        expect(Buffer.byteLength(body)).toBeGreaterThan(262144);
      } else expect(Buffer.byteLength(body)).toBeLessThan(262144);

      await observePreviewTransport(page);
      await initialize(page, owned.url);
      const resources = [];
      page.on('request', request => {
        const url = new URL(request.url());
        if (url.searchParams.has('native-quote-probe')) resources.push(url.pathname + url.search);
      });
      // This real, allowed image proves that CSP and the request observer would
      // allow/detect the same public asset if hostile markup reached live DOM.
      const control = '/static/themes/fade.png?native-quote-probe=healthy';
      expect(await page.evaluate(src => new Promise(resolve => {
        const image = new Image();
        image.onload = () => resolve(true); image.onerror = () => resolve(false);
        image.src = src;
      }), control)).toBe(true);
      expect(resources).toEqual([control]);

      const before = await page.evaluate(() => ({ ...window.ownedPreviewWorkers }));
      const pattern = `**${remote.previewPath}`;
      await page.route(pattern, route => route.fulfill({ contentType: 'application/json', body }));
      await quote(page, reply, remote).hover();
      // Production transport aborts its own signal when it settles, including
      // validation failures. Wait for that completion before asserting absence.
      await expect.poll(() => page.evaluate(() => window.ownedPreviewFetches.at(-1)?.aborted)).toBe(true);
      await expect(page.locator('#quote-preview .postMessage')).toHaveCount(0);
      expect(await page.evaluate(() => window.ownedPreviewCommits)).toEqual([]);
      expect(await page.evaluate(() => window.quotePreviewInjected)).toBeUndefined();
      expect(resources).toEqual([control]);
      const workers = await page.evaluate(() => ({ ...window.ownedPreviewWorkers }));
      expect(workers.started - before.started).toBe(defect === 'UTF-8 byte ceiling' ? 0 : 1);
      expect(workers.terminated - before.terminated).toBe(workers.started - before.started);

      await leaveQuote(page);
      await page.unroute(pattern);
      await page.waitForTimeout(350);
      await quote(page, reply, remote).hover();
      await expectPreview(page, remote, 'Healthy persisted recovery target');
      expect(await page.evaluate(marker => window.ownedPreviewCommits.every(text => !text.includes(marker)), rejectedMarker)).toBe(true);
      expect(resources).toEqual([control]);
    });
  }

  test('a transient HTTP failure preserves the link and a later hover fetches a fresh real response', async ({ page, owned }) => {
    const remote = await owned.createThread('test', 'Healthy target after transient failure');
    const reply = await owned.reply(`>>>/test/${remote.id}\nTransient failure quote`);
    await observePreviewTransport(page);
    await initialize(page, owned.url);
    const pattern = `**${remote.previewPath}`, requests = network(page);
    await page.route(pattern, route => route.fulfill({ status: 503, body: 'Owned transient failure' }));
    await quote(page, reply, remote).hover();
    await expect.poll(() => page.evaluate(() => window.ownedPreviewFetches.at(-1)?.aborted)).toBe(true);
    await expect(page.locator('#quote-preview .postMessage')).toHaveCount(0);
    await expect(quote(page, reply, remote)).toHaveAttribute('href', `/test/post/${remote.id}`);
    await leaveQuote(page); await page.unroute(pattern); await page.waitForTimeout(350);
    await quote(page, reply, remote).hover();
    await expectPreview(page, remote, 'Healthy target after transient failure');
    expect(requests.map(value => value.url())).toEqual([`${origin}${remote.previewPath}`, `${origin}${remote.previewPath}`]);
  });
});
