import { test as base, expect } from '@playwright/test';
import { openWatcherSettings } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';
const mobileAgent = 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36';
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];
const absent = '9223372036854775807';
const enabled = { inlineQuotes: true, quotePreview: false };
const trailingLines = Array.from({ length: 26 }, (_, index) => `Owned inline trailing line ${index + 1}`).join('\n');
const copiedMarker = 'OWNED_INLINE_COPY_ONLY';

const test = base.extend({
  owned: async ({ request, context }, use) => {
    const password = 'owned-native-inline-password', threads = [];
    let last;
    async function write(board, resto, com, { tracked = false, subject = '' } = {}) {
      const client = tracked ? context.request : request;
      const response = await client.post(`/${board}/post`, {
        headers: { Origin: origin }, maxRedirects: 0,
        form: { resto, com, password, ...(resto === '0' ? { sub: subject } : {}), ...(tracked ? { track: '1' } : {}) },
      });
      expect(response.status(), `Owned inline fixture must persist through the real posting form${response.status() === 303 ? '' : `: ${await response.text()}`}`).toBe(303);
      const ids = response.headers().location?.match(/\/thread\/([0-9]+)#p([0-9]+)$/);
      expect(ids, 'The real posting redirect supplies exact thread and post identities').not.toBeNull();
      last = ids[2];
      return last;
    }
    async function createThread(board = 'demo', comment = 'Owned inline original post', options) {
      const id = await write(board, '0', comment, options);
      const thread = { board, id, url: `/${board}/thread/${id}`, path: `/_watch/${board}/thread/${id}/posts`,
        reply: (com, options) => write(board, id, com, options) };
      threads.push(thread);
      return thread;
    }
    try {
      await use({ ...await createThread(), createThread, password,
        // Predict only consecutive posts in this exclusive serial fixture lane;
        // every self/cycle fixture checks the returned receipt IDs.
        nextId: (offset = 1) => String(BigInt(last) + BigInt(offset)),
      });
    } finally {
      for (const { board, id } of threads.reverse()) {
        expect((await request.post(`/${board}/delete`, {
          headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
        })).status(), 'Cleanup removes only this case\'s owned threads').toBe(303);
      }
    }
  },
});

const post = (thread, id = thread.id) => ({ board: thread.board, id, url: thread.url,
  path: `/_watch/${thread.board}/post/${id}` });
const quoteText = (target, board = 'demo') => target.board === board ? `>>${target.id}` : `>>>/${target.board}/${target.id}`;
const quoteSelector = target => `a.quotelink[href="/${target.board}/post/${target.id}"]`;
const originalQuote = (page, source, target) => page.locator(`#m${source} ${quoteSelector(target)}:not(.inlined a)`);
const backlink = (page, owner, target) => page.locator(`#bl_${owner} > span > a.quotelink[href="${target.url}#p${target.id}"]`);
// Find the nearest observable panel around its canonical post-number link. This
// works whether the renderer uses the post itself or an outer inline wrapper.
const inlineFor = (scope, target) => scope.locator(`.inlined a.postNum[href="${target.url}#p${target.id}"]`)
  .locator('xpath=ancestor::*[contains(concat(" ", normalize-space(@class), " "), " inlined ")][1]');
const rule = (pattern, changes = {}) => ({ type: 2, pattern, boards: 'demo', active: true,
  auto: false, hide: false, color: '#ff0000', ...changes });

async function initialize(page, url, settings = enabled, rules) {
  await page.addInitScript(({ settings, rules }) => {
    if (localStorage.getItem('4chan-settings') === null) localStorage.setItem('4chan-settings', JSON.stringify(settings));
    if (rules !== undefined && localStorage.getItem('4chan-filters') === null) localStorage.setItem('4chan-filters', JSON.stringify(rules));
  }, { settings, rules });
  await page.goto(new URL(url, origin).href);
  await expect(page.locator('#settingsWindowLink:visible, #settingsWindowLinkMobile:visible')).toBeVisible();
}

async function saveSettings(page, values) {
  const dialog = await openWatcherSettings(page);
  await dialog.locator('#settings-expand-all').click();
  for (const [key, value] of Object.entries(values)) await dialog.locator(`.menuOption[data-option="${key}"]`).setChecked(value);
  await Promise.all([page.waitForEvent('load'), dialog.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
}

function network(page) {
  const requests = [];
  page.on('request', request => { if (['fetch', 'xhr'].includes(request.resourceType())) requests.push(request); });
  return requests;
}

async function expectUniqueIds(page) {
  expect(await page.locator('[id]').evaluateAll(nodes => nodes.length === new Set(nodes.map(node => node.id)).size)).toBe(true);
}

async function expectInline(scope, target, text) {
  const panel = inlineFor(scope, target);
  await expect(panel).toHaveCount(1);
  await expect(panel).toBeVisible();
  await expect(panel.locator('.postMessage').first()).toContainText(text);
  await expect(panel.locator('form, input, textarea, button, details, script, iframe, object, embed')).toHaveCount(0);
  return panel;
}

async function hideReply(page, id) {
  await page.getByRole('button', { name: `Post menu for post ${id}`, exact: true }).click();
  await page.getByRole('menuitem', { name: 'Hide post', exact: true }).click();
  await expect(page.locator(`#pc${id}`)).toHaveClass(/post-hidden/);
}

test.describe('unmodified persisted inline quotes', () => {
  test('inline defaults off and its actual desktop setting survives navigation and disableAll', async ({ page, owned }) => {
    const target = post(owned), source = await owned.reply(`>>${owned.id}\nDefault inline source`);
    await initialize(page, owned.url, { quotePreview: false });
    const dialog = await openWatcherSettings(page);
    await dialog.locator('#settings-expand-all').click();
    await expect(dialog.getByLabel('Inline quote links', { exact: true })).not.toBeChecked();
    await dialog.getByRole('button', { name: 'Close settings', exact: true }).click();
    await originalQuote(page, source, target).click();
    await expect(page).toHaveURL(`${origin}${owned.url}#p${owned.id}`);
    await expect(page.locator('.inlined')).toHaveCount(0);

    await saveSettings(page, { inlineQuotes: true });
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).inlineQuotes)).toBe(true);
    await originalQuote(page, source, target).click();
    await expectInline(page, target, 'Owned inline original post');
    await page.goto('/demo/');
    await originalQuote(page, source, target).click();
    await expectInline(page, target, 'Owned inline original post');
    await saveSettings(page, { disableAll: true });
    await expect(page.locator('.inlined')).toHaveCount(0);
    await originalQuote(page, source, target).click();
    await expect(page).toHaveURL(`${origin}${owned.url}#p${owned.id}`);
    await expect(page.locator('.inlined')).toHaveCount(0);
    await saveSettings(page, { disableAll: false, inlineQuotes: false });
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).inlineQuotes)).toBe(false);
  });

  test('a local copy preserves original forms, entered values, identities and visibility across collapse', async ({ page, owned }) => {
    const target = post(owned, await owned.reply('Owned local target\n>green target\n<script>inlineEscapeProbe()</script>'));
    const source = await owned.reply(`>>${target.id}\nLocal source`);
    await initialize(page, owned.url);
    await page.locator(`#p${target.id} .postActions > summary`).click();
    await page.locator(`#delete${target.id}`).fill('owned-inline-private-value');
    await page.locator(`#report${target.id}`).fill('Owned unsent report value');
    await page.locator(`#p${target.id}`).evaluate(node => {
      window.inlineOriginalPost = node;
      window.inlineOriginalFields = [...node.querySelectorAll('input')].map(field => ({ field, value: field.value }));
      window.inlineOriginalForms = [...document.forms];
      window.inlineOriginalIds = [...document.querySelectorAll('[id]')].map(element => ({ element, id: element.id }));
    });
    const link = originalQuote(page, source, target), requests = network(page);
    await link.evaluate(node => { window.inlineOriginalAnchor = node; });
    await link.click();
    const panel = await expectInline(page, target, 'Owned local target');
    await expect(panel.locator('.postMessage')).toContainText('<script>inlineEscapeProbe()</script>');
    await expect(panel.locator('.highlight, .highlight-anti')).toHaveCount(0);
    await expect(link).toHaveClass(/linkfade/);
    await expect(page.locator(`#m${target.id}`)).toBeVisible();
    await expect(page.locator(`#delete${target.id}`)).toHaveValue('owned-inline-private-value');
    await expectUniqueIds(page);
    await link.click();
    await expect(page.locator('.inlined')).toHaveCount(0);
    await expect(link).not.toHaveClass(/linkfade/);
    expect(await page.evaluate(id => ({
      post: document.getElementById(`p${id}`) === window.inlineOriginalPost,
      fields: window.inlineOriginalFields.every(({ field, value }) => field.isConnected && field.value === value),
      forms: [...document.forms].every((form, index) => form === window.inlineOriginalForms[index])
        && document.forms.length === window.inlineOriginalForms.length,
      ids: window.inlineOriginalIds.every(({ element, id }) => document.getElementById(id) === element),
      anchor: window.inlineOriginalAnchor.isConnected,
    }), target.id)).toEqual({ post: true, fields: true, forms: true, ids: true, anchor: true });
    expect(requests).toEqual([]);
  });

  test('actual plain, greentext and spoiler sources place copies outside their rendered wrappers', async ({ page, owned }) => {
    // Newly posted /test/ rows enable spoilers; /demo/ deliberately does not.
    const thread = await owned.createThread('test', 'Owned placement target'), target = post(thread);
    const definitions = [
      { text: `>>${target.id}\nPlain source`, parent: 'BLOCKQUOTE', before: 'A' },
      { text: `>green source >>${target.id}`, parent: 'SPAN', before: 'SPAN' },
      { text: `[spoiler]>>${target.id}[/spoiler]\nSpoiler source`, parent: 'S', before: 'S' },
    ];
    const sources = [];
    for (const definition of definitions) sources.push({ ...definition, id: await thread.reply(definition.text) });
    await initialize(page, thread.url);
    for (const source of sources) {
      const link = originalQuote(page, source.id, target);
      expect(await link.evaluate(node => node.parentElement.tagName)).toBe(source.parent);
      if (source.parent === 'S') await expect(page.locator(`#m${source.id} > s > a.quotelink`)).toHaveCount(1);
      await link.click();
      const panel = await expectInline(page.locator(`#m${source.id}`), target, 'Owned placement target');
      expect(await panel.evaluate(node => ({ parent: node.parentElement.id, preceding: node.previousElementSibling?.tagName,
        spoiler: !!node.parentElement.closest('s,.spoiler'), green: !!node.parentElement.closest('.quote') })))
        .toEqual({ parent: `m${source.id}`, preceding: source.before, spoiler: false, green: false });
      await link.click();
      await expect(page.locator('.inlined')).toHaveCount(0);
    }
  });

  for (const board of ['demo', 'test']) {
    test(`a persisted ${board === 'demo' ? 'same-board remote' : 'cross-board remote'} quote uses only the bounded one-post endpoint`, async ({ page, owned }) => {
      const remoteThread = await owned.createThread(board, 'Owned remote inline target');
      await remoteThread.reply('Unrequested sibling must stay outside the inline copy');
      const remote = post(remoteThread), source = await owned.reply(`${quoteText(remote)}\nRemote inline source`);
      await initialize(page, owned.url);
      await expect(page.locator(`#p${remote.id}`)).toHaveCount(0);
      const requests = network(page), response = page.waitForResponse(`${origin}${remote.path}`);
      const link = originalQuote(page, source, remote), href = await link.getAttribute('href'), label = await link.textContent();
      await link.click();
      expect((await response).status()).toBe(200);
      const panel = await expectInline(page, remote, 'Owned remote inline target');
      await expect(panel).not.toContainText('Unrequested sibling');
      await expect(page.locator('#quote-preview')).toHaveCount(0);
      await expect(link).toHaveAttribute('href', href);
      await expect(link).toHaveText(label);
      expect(requests.map(request => new URL(request.url()).pathname)).toEqual([remote.path]);
      expect(requests[0].method()).toBe('GET');
      await expectUniqueIds(page);
      await link.click();
      await expect(page.locator('.inlined')).toHaveCount(0);
      await link.click();
      await expectInline(page, remote, 'Owned remote inline target');
      expect(requests.map(request => new URL(request.url()).pathname)).toEqual([remote.path, remote.path]);
    });
  }

  test('different original links to the same post expand and collapse independently', async ({ page, owned }) => {
    const target = post(owned), first = await owned.reply(`>>${owned.id}\nFirst independent source`);
    const second = await owned.reply(`>>${owned.id}\nSecond independent source`);
    await initialize(page, owned.url);
    const a = originalQuote(page, first, target), b = originalQuote(page, second, target);
    await a.click();
    await expectInline(page.locator(`#m${first}`), target, 'Owned inline original post');
    await b.click();
    await expectInline(page.locator(`#m${second}`), target, 'Owned inline original post');
    await expect(page.locator('.inlined')).toHaveCount(2);
    await a.click();
    await expect(page.locator(`#m${first} .inlined`)).toHaveCount(0);
    await expectInline(page.locator(`#m${second}`), target, 'Owned inline original post');
    await expect(b).toHaveClass(/linkfade/);
    await b.click();
    await expect(page.locator('.inlined')).toHaveCount(0);
    await expectUniqueIds(page);
  });

  test('a direct self-quote navigates normally while an ancestor cycle cannot create another copy', async ({ page, owned }) => {
    const firstId = owned.nextId(), secondId = owned.nextId(2);
    const first = post(owned, await owned.reply(`>>${firstId}\n>>${secondId}\nCycle first post`));
    const second = post(owned, await owned.reply(`>>${firstId}\nCycle second post`));
    expect(first.id).toBe(firstId);
    expect(second.id).toBe(secondId);
    const source = await owned.reply(`>>${first.id}\nCycle entry source`);
    await initialize(page, owned.url);
    await originalQuote(page, first.id, first).click();
    await expect(page).toHaveURL(`${origin}${first.url}#p${first.id}`);
    await expect(page.locator('.inlined')).toHaveCount(0);
    const outerLink = originalQuote(page, source, first), requests = network(page);
    await outerLink.click();
    const outer = await expectInline(page.locator(`#m${source}`), first, 'Cycle first post');
    await outer.locator(quoteSelector(second)).click();
    const inner = await expectInline(outer, second, 'Cycle second post');
    const cycle = inner.locator(quoteSelector(first)), location = page.url();
    await cycle.click();
    await expect(page.locator('.inlined')).toHaveCount(2);
    await expect(cycle).not.toHaveClass(/linkfade/);
    await expect(page).toHaveURL(location);
    expect(requests).toEqual([]);
    await outerLink.click();
    await expect(page.locator('.inlined')).toHaveCount(0);
  });

  test('the maximum positive i64 quote receives a real unavailable response and can be dismissed and retried', async ({ page, owned }) => {
    const target = post(owned, absent), source = await owned.reply(`>>${absent}\nOwned missing target source`);
    await initialize(page, owned.url);
    const link = originalQuote(page, source, target), requests = network(page);
    const missing = page.waitForResponse(`${origin}${target.path}`);
    await link.click();
    expect((await missing).status()).toBe(404);
    await expect(page.locator('.inlined')).toHaveText('This post or thread is unavailable.');
    await expect(page.locator('.inlined .postMessage')).toHaveCount(0);
    await expect(link).toHaveAttribute('href', `/demo/post/${absent}`);
    await link.click();
    await expect(page.locator('.inlined')).toHaveCount(0);
    await expect(link).not.toHaveClass(/deadlink|linkfade/);
    const retry = page.waitForResponse(`${origin}${target.path}`);
    await link.click();
    expect((await retry).status()).toBe(404);
    await expect(page.locator('.inlined')).toHaveText('This post or thread is unavailable.');
    expect(requests.map(request => new URL(request.url()).pathname)).toEqual([target.path, target.path]);
  });

  test('nested expansion collapses recursively and a new local copy omits expansions already in the original', async ({ page, owned }) => {
    const leaf = post(owned, await owned.reply('Owned nesting leaf'));
    const middle = post(owned, await owned.reply(`>>${leaf.id}\nOwned nesting middle`));
    const source = await owned.reply(`>>${middle.id}\nOwned nesting source`);
    await initialize(page, owned.url);
    const originalMiddle = originalQuote(page, middle.id, leaf), outerLink = originalQuote(page, source, middle);
    await originalMiddle.click();
    await expectInline(page.locator(`#m${middle.id}`), leaf, 'Owned nesting leaf');
    await outerLink.click();
    const outer = await expectInline(page.locator(`#m${source}`), middle, 'Owned nesting middle');
    await expect(outer.locator('.inlined')).toHaveCount(0);
    const innerLink = outer.locator(quoteSelector(leaf));
    await expect(innerLink).not.toHaveClass(/linkfade/);
    await innerLink.click();
    await expectInline(outer, leaf, 'Owned nesting leaf');
    await expect(page.locator('.inlined')).toHaveCount(3);
    await outerLink.click();
    await expect(page.locator(`#m${source} .inlined`)).toHaveCount(0);
    await expectInline(page.locator(`#m${middle.id}`), leaf, 'Owned nesting leaf');
    await originalMiddle.click();
    await expect(page.locator('.inlined')).toHaveCount(0);
    await expect(page.locator('.linkfade')).toHaveCount(0);
  });

  test('desktop backlinks prepend independent copies and restore their shared original after the last collapse', async ({ page, owned }) => {
    const firstOwner = await owned.reply('First backlink owner'), secondOwner = await owned.reply('Second backlink owner');
    const target = post(owned, await owned.reply(`>>${firstOwner}\n>>${secondOwner}\nOriginal hidden only by inline copies`));
    await initialize(page, owned.url);
    const first = backlink(page, firstOwner, target), second = backlink(page, secondOwner, target);
    await first.click();
    const panel = await expectInline(page.locator(`#m${firstOwner}`), target, 'Original hidden only by inline copies');
    expect(await panel.evaluate(node => node.parentElement.firstElementChild === node)).toBe(true);
    await expect(page.locator(`#m${target.id}`)).toBeHidden();
    await second.click();
    await expectInline(page.locator(`#m${secondOwner}`), target, 'Original hidden only by inline copies');
    await first.click();
    await expect(page.locator(`#m${firstOwner} .inlined`)).toHaveCount(0);
    await expect(page.locator(`#m${target.id}`)).toBeHidden();
    await second.click();
    await expect(page.locator('.inlined')).toHaveCount(0);
    await expect(page.locator(`#m${target.id}`)).toBeVisible();
    await expect(first).not.toHaveClass(/linkfade/);
    await expect(second).not.toHaveClass(/linkfade/);
    await expectUniqueIds(page);
  });

  test('closing backlink copies preserves independent manual hiding and real filter hiding', async ({ page, context, owned }) => {
    const owner = await owned.reply('Owner remains clickable');
    const target = post(owned, await owned.reply(`>>${owner}\nPreserve independent hiding`));
    await initialize(page, owned.url, { ...enabled, filter: true }, []);
    await hideReply(page, target.id);
    const source = backlink(page, owner, target);
    await source.click();
    await expectInline(page.locator(`#m${owner}`), target, 'Preserve independent hiding');
    await source.click();
    await expect(page.locator(`#pc${target.id}`)).toHaveClass(/post-hidden/);
    await expect(page.locator(`#m${target.id}`)).toBeHidden();

    const filteredTarget = post(owned, await owned.reply(`>>${owner}\nSeparate filter hide needle`));
    // Navigation admits the new persisted source and retains manual storage.
    await page.reload();
    const other = await context.newPage();
    try {
      await other.goto(owned.url);
      const filteredSource = backlink(page, owner, filteredTarget);
      await filteredSource.click();
      await expectInline(page.locator(`#m${owner}`), filteredTarget, 'Separate filter hide needle');
      await other.evaluate(rules => localStorage.setItem('4chan-filters', JSON.stringify(rules)),
        [rule('Separate filter hide needle', { hide: true })]);
      await expect(page.locator(`#p${filteredTarget.id}`)).toHaveClass(/post-hidden/);
      await expectInline(page.locator(`#m${owner}`), filteredTarget, 'Separate filter hide needle');
      await filteredSource.click();
      await expect(page.locator(`#p${filteredTarget.id}`)).toHaveClass(/post-hidden/);
      await expect(page.locator(`#m${filteredTarget.id}`)).toBeHidden();
      await expect(page.locator(`#pc${target.id}`)).toHaveClass(/post-hidden/);
      await expect(page.locator(`#m${owner}`)).toBeVisible();
      await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
    } finally { await other.close(); }
  });

  test('mobile backlink copies stay adjacent and their original post and # navigation remain usable', async ({ browser, owned }) => {
    const owner = await owned.reply('Mobile backlink owner'), target = post(owned, await owned.reply(`>>${owner}\nMobile backlink target`));
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, userAgent: mobileAgent, isMobile: true, hasTouch: true });
    try {
      const page = await context.newPage();
      await initialize(page, owned.url, { inlineQuotes: true, quotePreview: true });
      const source = backlink(page, owner, target), row = page.locator(`#bl_${owner}.mobile`), requests = network(page);
      await source.tap();
      const panel = await expectInline(row, target, 'Mobile backlink target');
      expect(await panel.evaluate(node => !!node.closest('.backlink.mobile'))).toBe(true);
      await expect(page.locator('#quote-preview')).toHaveCount(0);
      expect(requests).toEqual([]);
      await expect(page.locator(`#m${owner} .inlined`)).toHaveCount(0);
      await expect(page.locator(`#m${target.id}`)).toBeVisible();
      expect(await source.evaluate(node => node.nextElementSibling?.matches('a.quoteLink'))).toBe(true);
      await expect(row.locator(':scope > span > a.quoteLink')).toHaveCount(1);
      await row.locator(':scope > span > a.quoteLink').tap();
      await expect(page).toHaveURL(`${origin}${target.url}#p${target.id}`);
    } finally { await context.close(); }
  });

  test('genuine mobile taps prioritize inline loading, preserve # navigation and retain preview fallback when disabled', async ({ browser, owned }) => {
    const remote = post(await owned.createThread('test', 'Mobile remote inline priority'));
    const source = await owned.reply(`${quoteText(remote)}\n${trailingLines}`);
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, userAgent: mobileAgent, isMobile: true, hasTouch: true });
    try {
      const page = await context.newPage(), other = await context.newPage();
      await initialize(page, owned.url, { inlineQuotes: true, quotePreview: true });
      const dialog = await openWatcherSettings(page);
      await dialog.locator('#settings-expand-all').click();
      await expect(dialog.getByLabel('Inline quote links', { exact: true })).toHaveCount(0);
      await dialog.getByRole('button', { name: 'Close settings', exact: true }).click();
      const link = originalQuote(page, source, remote), requests = network(page);
      await link.tap();
      await expectInline(page, remote, 'Mobile remote inline priority');
      await expect(page.locator('#quote-preview')).toHaveCount(0);
      expect(requests.map(request => new URL(request.url()).pathname)).toEqual([remote.path]);
      expect(await link.evaluate(node => node.nextElementSibling?.matches('a.quoteLink'))).toBe(true);
      await expect(page.locator(`#m${source} > a.quoteLink`)).toHaveCount(1);
      await link.tap();
      await expect(page.locator('.inlined')).toHaveCount(0);

      // Deliver a real storage event from another tab; this layout omits the checkbox.
      await other.goto(`${origin}${owned.url}`);
      await other.evaluate(() => {
        const settings = JSON.parse(localStorage.getItem('4chan-settings'));
        settings.inlineQuotes = false;
        localStorage.setItem('4chan-settings', JSON.stringify(settings));
      });
      await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).inlineQuotes)).toBe(false);
      await link.tap();
      await expect(page.locator('#quote-preview .postMessage')).toContainText('Mobile remote inline priority');
      await expect(page.locator('.inlined')).toHaveCount(0);
      expect(requests.map(request => new URL(request.url()).pathname)).toEqual([remote.path, remote.path]);
      await page.locator(`#m${source} > a.quoteLink`).tap();
      await expect(page).toHaveURL(`${origin}${remote.url}#p${remote.id}`);
    } finally { await context.close(); }
  });

  test('Control, Meta, Alt and keyboard activation inline while Shift and non-primary input keep navigation', async ({ page, context, owned }) => {
    const target = post(owned, await owned.reply('Modifier target')), source = await owned.reply(`>>${target.id}\nModifier source`);
    await initialize(page, owned.url);
    const link = originalQuote(page, source, target), location = page.url();
    for (const modifier of ['Control', 'Meta', 'Alt']) {
      await link.click({ modifiers: [modifier] });
      await expectInline(page, target, 'Modifier target');
      await expect(page).toHaveURL(location);
      expect(context.pages()).toHaveLength(1);
      await link.click();
      await expect(page.locator('.inlined')).toHaveCount(0);
    }
    await link.focus();
    await page.keyboard.press('Enter');
    await expectInline(page, target, 'Modifier target');
    await link.click();
    await link.click({ modifiers: ['Shift'] });
    await expect(page).toHaveURL(`${origin}${target.url}#p${target.id}`);
    await expect(page.locator('.inlined')).toHaveCount(0);
    const opened = context.waitForEvent('page');
    await originalQuote(page, source, target).click({ button: 'middle' });
    const ordinary = await opened;
    try { await expect(ordinary).toHaveURL(`${origin}${target.url}#p${target.id}`); }
    finally { await ordinary.close(); }
    await expect(page.locator('.inlined')).toHaveCount(0);
    await originalQuote(page, source, target).click({ button: 'right' });
    await expect(page.locator('.inlined')).toHaveCount(0);
    await page.keyboard.press('Escape');
  });

  test('inline body text does not change original filter matches, tracked suffixes or backlink membership', async ({ page, context, owned }) => {
    const tracked = await owned.createThread('demo', 'Tracked inline OP', { tracked: true });
    const target = post(tracked, await tracked.reply(`>>${tracked.id}\n>>${absent}\n${copiedMarker}`));
    const source = await tracked.reply(`>>${target.id}\nOriginal source without copied marker`);
    const literal = await tracked.reply(`Literal control ${copiedMarker}`);
    await initialize(page, tracked.url, { ...enabled, filter: true }, [rule(copiedMarker)]);
    const originalOP = originalQuote(page, target.id, post(tracked));
    await expect(originalOP).toHaveText(`>>${tracked.id} (You) (OP)`);
    await expect(page.locator(`#m${target.id} a[href="/demo/post/${absent}"]`)).toHaveText(`>>${absent} →`);
    const rows = await page.locator('.board .backlink:not(.inlined .backlink)').evaluateAll(nodes => nodes.map(node => node.outerHTML));
    await originalQuote(page, source, target).click();
    await expectInline(page.locator(`#m${source}`), target, copiedMarker);
    await expect(page.locator(`#p${target.id}, #p${literal}`)).toHaveClass([/filter-hl/, /filter-hl/]);
    await expect(page.locator(`#p${source}`)).not.toHaveClass(/filter-hl|post-hidden/);
    const other = await context.newPage();
    try {
      await other.goto(tracked.url);
      await other.evaluate(() => localStorage.setItem('4chan-filters', JSON.stringify([
        { type: 2, pattern: '/\\(You\\)/', boards: 'demo', active: true, hide: false, color: '#ff0000' },
      ])));
      await expect(page.locator(`#p${target.id}`)).toHaveClass(/filter-hl/);
      await expect(page.locator(`#p${literal}`)).not.toHaveClass(/filter-hl/);
      await expect(page.locator(`#p${source}`)).not.toHaveClass(/filter-hl|post-hidden/);
      await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
      await expect(originalOP).toHaveText(`>>${tracked.id} (You) (OP)`);
      expect(await page.locator('.board .backlink:not(.inlined .backlink)').evaluateAll(nodes => nodes.map(node => node.outerHTML))).toEqual(rows);
      await expectUniqueIds(page);
    } finally { await other.close(); }
  });

  test('an inline copy in the OP leaves watcher labels and original read positions unchanged', async ({ page, owned }) => {
    const predicted = owned.nextId();
    const thread = await owned.createThread('demo', `>>${predicted}\nOriginal watcher label`);
    // The opening post intentionally quotes itself first; use its reply backlink
    // to prepend copied text ahead of the OP's original comment.
    expect(thread.id).toBe(predicted);
    const target = post(thread, await thread.reply(`>>${thread.id}\n${copiedMarker}`));
    await initialize(page, thread.url, { ...enabled, threadWatcher: true });
    const control = page.locator('.threadNav.desktop [data-cmd="watch"]').first();
    const row = page.locator(`#watch-${thread.id}-demo`);
    await control.click();
    await expect(row).toHaveCount(1);
    const saved = await page.evaluate(key => JSON.parse(localStorage.getItem('4chan-watch'))[key], `${thread.id}-demo`);
    await control.click();
    await expect(row).toHaveCount(0);
    await backlink(page, thread.id, target).click();
    await expectInline(page.locator(`#m${thread.id}`), target, copiedMarker);
    await control.click();
    await expect(row).toHaveCount(1);
    await expect.poll(() => page.evaluate(key => JSON.parse(localStorage.getItem('4chan-watch'))[key], `${thread.id}-demo`)).toEqual(saved);
    await expect(page.locator(`#watch-${thread.id}-demo`)).not.toContainText(copiedMarker);
  });

  for (const theme of themes) {
    test(`${theme} keeps local inline content readable and within the viewport at desktop and mobile widths`, async ({ page, context, owned }) => {
      const target = post(owned, await owned.reply('Theme inline target\n>owned green text'));
      const source = await owned.reply(`>>${target.id}\nTheme inline source`);
      await context.addCookies([{ name: 'board-theme-ws', value: theme, url: origin, httpOnly: true, sameSite: 'Lax' }]);
      await initialize(page, owned.url);
      for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 900 });
        const link = originalQuote(page, source, target);
        await link.click();
        const panel = await expectInline(page, target, 'Theme inline target');
        const box = await panel.boundingBox();
        expect(box.width).toBeGreaterThan(0);
        expect(box.height).toBeGreaterThan(0);
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(width + 1);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
        const originalStyle = await page.locator(`#m${target.id}`).evaluate(node => ({ color: getComputedStyle(node).color,
          green: getComputedStyle(node.querySelector('.quote')).color }));
        await expect(panel.locator('.postMessage')).toHaveCSS('color', originalStyle.color);
        await expect(panel.locator('.quote')).toHaveCSS('color', originalStyle.green);
        await expectUniqueIds(page);
        await link.click();
        await expect(page.locator('.inlined')).toHaveCount(0);
      }
    });
  }
});

async function observeTransport(page, abortLogKey = null) {
  await page.addInitScript(abortLogKey => {
    const fetch = window.fetch;
    window.inlineFetches = [];
    window.inlineCommits = [];
    window.fetch = function (input, options) {
      const path = new URL(typeof input === 'string' || input instanceof URL ? input : input.url, location.href).pathname;
      let entry;
      if (/^\/_watch\/[a-z0-9]+\/post\/[0-9]+$/.test(path)) {
        const signal = options?.signal ?? input?.signal;
        entry = { path, aborted: signal?.aborted === true, settled: false };
        window.inlineFetches.push(entry);
        signal?.addEventListener('abort', () => {
          entry.aborted = true;
          if (abortLogKey) {
            // A synchronous signal witness survives actual document replacement;
            // the browser protocol need not emit requestfailed for a retired page.
            const aborted = JSON.parse(sessionStorage.getItem(abortLogKey) || '[]');
            if (aborted.length < 128) sessionStorage.setItem(abortLogKey, JSON.stringify([...aborted, path]));
          }
        }, { once: true });
      }
      const result = Reflect.apply(fetch, this, [input, options]);
      if (entry) result.then(() => { entry.settled = true; }, () => { entry.settled = true; });
      return result;
    };
    new MutationObserver(() => {
      for (const message of document.querySelectorAll('.inlined .postMessage')) {
        const value = message.textContent;
        if (window.inlineCommits.length < 128 && !window.inlineCommits.includes(value)) window.inlineCommits.push(value);
      }
    }).observe(document, { childList: true, subtree: true });
  }, abortLogKey);
}

async function holdResponse(page, path) {
  let entered, failed, release, complete, active = false, released = false;
  const arrived = new Promise((resolve, reject) => { entered = resolve; failed = reject; });
  const gate = new Promise(resolve => { release = resolve; });
  const finished = new Promise(resolve => { complete = resolve; });
  const pattern = `**${path}`;
  const handler = async route => {
    active = true;
    try {
      // Keep the real body and headers. This fixture changes response timing only.
      const response = await route.fetch();
      entered();
      await gate;
      await route.fulfill({ response }).catch(() => {}); // Cancellation may retire the originating page.
    } catch (error) { failed(error); }
    finally { complete(); }
  };
  await page.route(pattern, handler, { times: 1 });
  return { arrived, async release() {
    if (released) return;
    released = true; release();
    await page.unroute(pattern, handler);
    if (active) await finished;
  } };
}

async function expectAborted(page, path) {
  await expect.poll(() => page.evaluate(path => window.inlineFetches.some(entry => entry.path === path && entry.aborted), path)).toBe(true);
}

test.describe('held genuine responses and real cross-tab cancellation', () => {
  test('a pending remote inline shows loading and repeated activation sends one request', async ({ page, owned }) => {
    const remote = post(await owned.createThread('test', 'Released inline loading target'));
    const source = await owned.reply(`${quoteText(remote)}\nLoading source`);
    await observeTransport(page);
    await initialize(page, owned.url);
    const requests = network(page), held = await holdResponse(page, remote.path);
    try {
      const link = originalQuote(page, source, remote);
      await link.click();
      await held.arrived;
      await expect(page.locator('.inlined')).toHaveCount(1);
      await expect(page.locator('.inlined')).toContainText(/loading/i);
      await expect(page.locator('.inlined .postMessage')).toHaveCount(0);
      await link.click();
      await expect(page.locator('.inlined')).toHaveCount(1);
      expect(requests.map(request => new URL(request.url()).pathname)).toEqual([remote.path]);
      expect(await page.evaluate(() => window.inlineFetches[0].aborted)).toBe(false);
      await held.release();
      await expectInline(page, remote, 'Released inline loading target');
      await link.click();
      await expect(page.locator('.inlined')).toHaveCount(0);
    } finally { await held.release(); }
  });

  for (const setting of ['inlineQuotes', 'disableAll']) {
    test(`a real ${setting} save cancels a pending inline and rejects its late response`, async ({ page, context, owned }) => {
      const remote = post(await owned.createThread('test', 'Settings-cancelled inline target'));
      const source = await owned.reply(`${quoteText(remote)}\nCross-tab pending source`);
      await observeTransport(page);
      await initialize(page, owned.url);
      const other = await context.newPage();
      await other.goto(owned.url);
      const held = await holdResponse(page, remote.path);
      try {
        const link = originalQuote(page, source, remote);
        await link.evaluate(node => { window.pendingInlineAnchor = node; window.pendingInlineDocument = document; });
        await link.click();
        await held.arrived;
        await expect(page.locator('.inlined')).toContainText(/loading/i);
        await saveSettings(other, { [setting]: setting === 'disableAll' });
        await expectAborted(page, remote.path);
        await expect(page.locator('.inlined')).toHaveCount(0);
        await expect(link).not.toHaveClass(/linkfade/);
        await held.release();
        expect(await page.evaluate(() => window.inlineCommits)).toEqual([]);
        expect(await link.evaluate(node => node === window.pendingInlineAnchor && document === window.pendingInlineDocument)).toBe(true);
        await saveSettings(other, { inlineQuotes: true, disableAll: false });
        await link.click();
        await expectInline(page, remote, 'Settings-cancelled inline target');
        await saveSettings(other, { inlineQuotes: false });
        await expect(page.locator('.inlined')).toHaveCount(0);
        await expect(link).not.toHaveClass(/linkfade/);
      } finally { await held.release(); await other.close(); }
    });
  }

  test('hiding the original source through a real filter cancels loading and allows a later healthy expansion', async ({ page, context, owned }) => {
    const remote = post(await owned.createThread('test', 'Filtered source late target'));
    const source = await owned.reply(`${quoteText(remote)}\nHide pending inline source needle`);
    await observeTransport(page);
    await initialize(page, owned.url, { ...enabled, filter: true }, []);
    const other = await context.newPage();
    await other.goto(owned.url);
    const held = await holdResponse(page, remote.path);
    try {
      await originalQuote(page, source, remote).click();
      await held.arrived;
      await other.evaluate(rules => localStorage.setItem('4chan-filters', JSON.stringify(rules)),
        [rule('Hide pending inline source needle', { hide: true })]);
      await expect(page.locator(`#p${source}`)).toHaveClass(/post-hidden/);
      await expect(page.locator(`#m${source}`)).toBeHidden();
      await expectAborted(page, remote.path);
      await held.release();
      await expect(page.locator('.inlined')).toHaveCount(0);
      expect(await page.evaluate(() => window.inlineCommits)).toEqual([]);
      await other.evaluate(() => localStorage.setItem('4chan-filters', '[]'));
      await expect(page.locator(`#m${source}`)).toBeVisible();
      await originalQuote(page, source, remote).click();
      await expectInline(page, remote, 'Filtered source late target');
      await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
    } finally { await held.release(); await other.close(); }
  });

  test('collapsing an outer local copy cancels its pending remote descendant and releases every owned effect', async ({ page, owned }) => {
    const remote = post(await owned.createThread('test', 'Late descendant must not return'));
    const middle = post(owned, await owned.reply(`${quoteText(remote)}\nLocal parent of remote request`));
    const source = await owned.reply(`>>${middle.id}\nOuter inline source`);
    await observeTransport(page);
    await initialize(page, owned.url);
    const outerLink = originalQuote(page, source, middle);
    await outerLink.click();
    const outer = await expectInline(page, middle, 'Local parent of remote request');
    const held = await holdResponse(page, remote.path);
    try {
      await outer.locator(quoteSelector(remote)).click();
      await held.arrived;
      await expect(outer.locator('.inlined')).toContainText(/loading/i);
      await outerLink.click();
      await expectAborted(page, remote.path);
      await held.release();
      await expect(page.locator('.inlined')).toHaveCount(0);
      await expect(page.locator('.linkfade')).toHaveCount(0);
      expect(await page.evaluate(() => window.inlineCommits.some(text => text.includes('Late descendant must not return')))).toBe(false);
      await outerLink.click();
      const fresh = await expectInline(page, middle, 'Local parent of remote request');
      await fresh.locator(quoteSelector(remote)).click();
      await expectInline(fresh, remote, 'Late descendant must not return');
    } finally { await held.release(); }
  });

  test('real page navigation retires a pending request and returning starts with fresh source ownership', async ({ page, owned }) => {
    const remote = post(await owned.createThread('test', 'Page-exit inline target'));
    const source = await owned.reply(`${quoteText(remote)}\nPage-exit source`);
    const abortLogKey = `owned-inline-page-exit-${remote.id}`;
    await observeTransport(page, abortLogKey);
    await initialize(page, owned.url);
    const held = await holdResponse(page, remote.path);
    try {
      await originalQuote(page, source, remote).click();
      await held.arrived;
      await expect(page.locator('.inlined')).toContainText(/loading/i);
      expect(await page.evaluate(key => sessionStorage.getItem(key), abortLogKey)).toBeNull();
      await page.goto('/demo/');
      await held.release();
      expect(await page.evaluate(key => JSON.parse(sessionStorage.getItem(key) || '[]'), abortLogKey)).toEqual([remote.path]);
      await expect(page.locator('.inlined')).toHaveCount(0);
      await page.goto(owned.url);
      await expect(page.locator('.inlined')).toHaveCount(0);
      await expect(originalQuote(page, source, remote)).not.toHaveClass(/linkfade/);
      await originalQuote(page, source, remote).click();
      await expectInline(page, remote, 'Page-exit inline target');
    } finally { await held.release(); }
  });
});

// Observe actual filter jobs and optionally delay sending them to the real
// release Worker. Worker results and server response contents are unchanged.
async function observeFilterJobs(page) {
  await page.addInitScript(() => {
    const OriginalWorker = window.Worker;
    const state = window.inlineFilterGate = { holding: false, jobs: [], release: null };
    state.release = () => {
      state.holding = false;
      for (const job of state.jobs) if (!job.sent && !job.terminated) { job.sent = true; job.send(); }
    };
    window.Worker = class extends OriginalWorker {
      constructor(...args) { super(...args); this.inlineJobs = []; }
      postMessage(raw, ...rest) {
        let value;
        try { value = typeof raw === 'string' ? JSON.parse(raw) : null; } catch { /* Other worker messages pass through. */ }
        if (value?.mode !== 'page' || !Array.isArray(value.posts)) return super.postMessage(raw, ...rest);
        const job = { posts: value.posts, sent: !state.holding, terminated: false,
          send: () => super.postMessage(raw, ...rest) };
        this.inlineJobs.push(job);
        if (state.jobs.length < 128) state.jobs.push(job);
        if (job.sent) job.send();
      }
      terminate() { for (const job of this.inlineJobs) job.terminated = true; return super.terminate(); }
    };
  });
}

test.describe('held real filter-worker timing during persisted updater work', () => {
  test('updater settlement excludes a displayed copy from original filter input, reply priority and watch acknowledgement', async ({ page, context, owned }) => {
    const thread = await owned.createThread('demo', 'Owned tracked update OP', { tracked: true });
    const target = post(thread, await thread.reply(`>>${thread.id}\n${copiedMarker}\n${trailingLines}`));
    await observeFilterJobs(page);
    await initialize(page, thread.url, { ...enabled, filter: true, threadWatcher: true },
      [rule(copiedMarker, { hide: false }), rule('Actual newly appended text')]);
    await expect(originalQuote(page, target.id, post(thread))).toHaveText(`>>${thread.id} (You) (OP)`);
    await expect(page.locator(`#p${target.id}`)).toHaveClass(/filter-hl/);
    await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
    await page.locator('.threadNav.desktop [data-cmd="watch"]').first().click();
    const source = await thread.reply(`>>${target.id}\nActual newly appended text`);
    // Freeze browser time while deliberately holding the public filter deadline,
    // then advance the updater's documented ten-second polling interval.
    const time = new Date('2026-09-15T18:00:00Z');
    await page.clock.install({ time });
    await page.clock.pauseAt(time);
    await page.evaluate(id => {
      window.inlineFilterGate.holding = true;
      window.inlineUpdateCompletion = null;
      document.addEventListener('4chanThreadUpdated', () => {
        const source = document.getElementById(`p${id}`);
        window.inlineUpdateCompletion = {
          copies: source.querySelectorAll('.inlined').length,
          highlighted: source.classList.contains('filter-hl'),
          notice: document.querySelector('.nativeFilterNotice').textContent,
          icon: document.querySelector('link[rel="shortcut icon"]').getAttribute('href'),
        };
      }, { once: true });
    }, source);
    await page.locator('.threadNav.desktop input[data-cmd="auto"]').first().check();
    await page.clock.runFor(10000);
    await expect(page.locator(`#m${source}`)).toBeVisible();
    await expect.poll(() => page.evaluate(id => window.inlineFilterGate.jobs.some(job => !job.sent && !job.terminated
      && job.posts.some(post => post.no === id)), source)).toBe(true);
    await originalQuote(page, source, target).click();
    await expectInline(page.locator(`#m${source}`), target, copiedMarker);
    expect(await page.evaluate(() => window.inlineUpdateCompletion)).toBeNull();
    const beforeChange = await page.evaluate(() => window.inlineFilterGate.jobs.length);
    const other = await context.newPage();
    try {
      await other.goto('/demo/');
      // Force a genuine replacement pass after expansion instead of assuming an
      // observer must re-filter when an owned copy leaves the input unchanged.
      await other.evaluate(rules => localStorage.setItem('4chan-filters', JSON.stringify(rules)),
        [rule(copiedMarker, { color: '#0000ff' }), rule('Actual newly appended text', { color: '#0000ff' })]);
      await page.bringToFront();
      await expect.poll(() => page.evaluate(() => window.inlineFilterGate.jobs.length)).toBeGreaterThan(beforeChange);
    } finally { await other.close(); }
    const inputs = await page.evaluate(id => window.inlineFilterGate.jobs.flatMap(job => job.posts.filter(post => post.no === id)), source);
    expect(inputs.length).toBeGreaterThan(1);
    for (const input of inputs) {
      expect(input.com).toContain('Actual newly appended text');
      expect(input.com).not.toContain(copiedMarker);
      expect(input.com).not.toContain('(You)');
      expect(input.filename).toBe('');
    }
    await page.evaluate(() => window.inlineFilterGate.release());
    await expect.poll(() => page.evaluate(() => window.inlineUpdateCompletion)).toEqual({ copies: 1, highlighted: true,
      notice: '', icon: '/static/notifications/favicon-ws-newfilters.ico' });
    await expect.poll(() => page.evaluate(key => String(JSON.parse(localStorage.getItem('4chan-watch'))[key][1]), `${thread.id}-demo`)).toBe(source);
    await expect(originalQuote(page, source, target)).toHaveText(`>>${target.id}`);
    await expect(page.locator(`#bl_${thread.id} > span > a.quotelink`)).toHaveCount(1);
    await expect(page.locator(`#bl_${target.id} > span > a.quotelink`)).toHaveCount(1);
    await page.locator('.threadNav.desktop input[data-cmd="auto"]').first().uncheck();
  });
});

test.describe('explicitly augmented DOM and substituted response controls', () => {
  test('a substituted HTTP failure stays visible until dismissed and a later activation loads the real post', async ({ page, owned }) => {
    const remote = post(await owned.createThread('test', 'Recovered real inline post'));
    const source = await owned.reply(`${quoteText(remote)}\nTransient inline failure source`);
    await initialize(page, owned.url);
    const pattern = `**${remote.path}`, requests = network(page), link = originalQuote(page, source, remote);
    await page.route(pattern, route => route.fulfill({ status: 503, body: 'Owned temporary failure' }), { times: 1 });
    await link.click();
    await expect(page.locator('.inlined')).toHaveText('Error: Quote could not be loaded.');
    await expect(link).toHaveAttribute('href', `/test/post/${remote.id}`);
    await link.click();
    await expect(page.locator('.inlined')).toHaveCount(0);
    await link.click();
    await expectInline(page, remote, 'Recovered real inline post');
    expect(requests.map(request => new URL(request.url()).pathname)).toEqual([remote.path, remote.path]);
  });

  test('unowned inline-shaped text remains filterable and a copied filename does not become its source filename', async ({ page, context, owned }) => {
    const target = post(owned, await owned.reply('Augmented filename target'));
    const source = await owned.reply(`>>${target.id}\nSource has no attachment`);
    const forged = await owned.reply('Original text before an unowned inline-shaped span');
    await observeFilterJobs(page);
    await initialize(page, owned.url, { ...enabled, filter: true }, [rule('/FORGED_INLINE_TEXT/')]);
    await page.locator(`#m${forged}`).evaluate(message => {
      const node = document.createElement('span'); node.className = 'inlined'; node.textContent = 'FORGED_INLINE_TEXT';
      message.append(node);
    });
    await expect(page.locator(`#p${forged}`)).toHaveClass(/filter-hl/);
    // Attachments remain an explicit DOM fixture; no media backend is claimed.
    await page.locator(`#p${target.id}`).evaluate((element, target) => {
      const file = document.createElement('div'); file.className = 'file';
      const text = document.createElement('p'), link = document.createElement('a');
      link.href = `${target.url}#p${target.id}`; link.textContent = 'owned-inline-file.png';
      text.append(link); file.append(text); element.querySelector('.postMessage').before(file);
    }, target);
    await originalQuote(page, source, target).click();
    const panel = await expectInline(page.locator(`#m${source}`), target, 'Augmented filename target');
    await expect(panel.locator('.file > p > a')).toHaveText('owned-inline-file.png');
    const other = await context.newPage();
    try {
      await other.goto(owned.url);
      await other.evaluate(rules => localStorage.setItem('4chan-filters', JSON.stringify(rules)),
        [rule('/owned-inline-file\\.png/', { type: 6 })]);
      await expect(page.locator(`#p${target.id}`)).toHaveClass(/filter-hl/);
      await expect(page.locator(`#p${forged}`)).not.toHaveClass(/filter-hl/);
      await expect(page.locator(`#p${source}`)).not.toHaveClass(/filter-hl|post-hidden/);
      await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
      const inputs = await page.evaluate(id => window.inlineFilterGate.jobs.flatMap(job => job.posts.filter(post => post.no === id)), source);
      expect(inputs.length).toBeGreaterThan(0);
      expect(inputs.every(input => input.filename === '')).toBe(true);
    } finally { await other.close(); }
  });

  for (const defect of ['unsafe resource', 'post identity', 'UTF-8 byte ceiling']) {
    test(`${defect} rejects the whole inline response before DOM/resource creation and permits a fresh retry`, async ({ page, request, owned }) => {
      const remote = post(await owned.createThread('test', 'Healthy inline response after rejection'));
      const source = await owned.reply(`${quoteText(remote)}\nResponse-validation source`);
      const response = await request.get(remote.path);
      expect(response.status()).toBe(200);
      const snapshot = await response.json(), marker = 'REJECTED_INLINE_PAYLOAD';
      let payload = marker;
      const probe = '/static/themes/fade.png?inline-resource=hostile';
      if (defect === 'unsafe resource') payload += `<img src="${probe}" onerror="window.inlineResourceExecuted=true">`;
      if (defect === 'post identity') snapshot.post.no = owned.id;
      if (defect === 'UTF-8 byte ceiling') payload += '\u6f22'.repeat(90000);
      snapshot.post.html = snapshot.post.html.replace('</blockquote>', `${payload}</blockquote>`);
      const body = JSON.stringify(snapshot);
      if (defect === 'UTF-8 byte ceiling') {
        expect(body.length).toBeLessThan(262144);
        expect(Buffer.byteLength(body)).toBeGreaterThan(262144);
      }
      await observeTransport(page);
      await initialize(page, owned.url);
      const resources = [];
      page.on('request', request => { const url = new URL(request.url()); if (url.searchParams.has('inline-resource')) resources.push(url.pathname + url.search); });
      const control = '/static/themes/fade.png?inline-resource=healthy';
      expect(await page.evaluate(src => new Promise(resolve => {
        const image = new Image(); image.onload = () => resolve(true); image.onerror = () => resolve(false); image.src = src;
      }), control)).toBe(true);
      const pattern = `**${remote.path}`, link = originalQuote(page, source, remote);
      await page.route(pattern, route => route.fulfill({ contentType: 'application/json', body }));
      await link.click();
      await expectAborted(page, remote.path);
      await expect(page.locator('.inlined .postMessage')).toHaveCount(0);
      await expect(page.locator('.inlined')).toHaveText('Error: Quote could not be loaded.');
      expect(await page.evaluate(() => window.inlineCommits)).toEqual([]);
      expect(await page.evaluate(() => window.inlineResourceExecuted)).toBeUndefined();
      expect(resources).toEqual([control]);
      await page.unroute(pattern);
      // The first activation closes the visible error; the next makes a new request.
      await link.click();
      await expect(page.locator('.inlined')).toHaveCount(0);
      await link.click();
      await expectInline(page, remote, 'Healthy inline response after rejection');
      expect(await page.evaluate(marker => window.inlineCommits.every(text => !text.includes(marker)), marker)).toBe(true);
      expect(resources).toEqual([control]);
      await expectUniqueIds(page);
    });
  }
});

async function inlineLimits(page) {
  const limits = await page.evaluate(async () => (await import('/static/native-backlinks.v1.js')).INLINE_LIMITS);
  for (const key of ['open', 'pending', 'depth', 'nodes', 'characters', 'quotes', 'pendingMs']) {
    expect(Number.isSafeInteger(limits[key]) && limits[key] > 0, `Missing finite inline ${key} contract`).toBe(true);
  }
  // Bound the fixture itself before creating actual posts or clicking copies.
  expect(limits.open).toBeLessThanOrEqual(32);
  expect(limits.pending).toBeLessThanOrEqual(16);
  expect(limits.depth).toBeLessThanOrEqual(16);
  return limits;
}

test.describe('finite admission with owned persisted posts', () => {
  test('open and nesting caps leave the first overflowing source on its ordinary navigation path', async ({ page, owned }) => {
    await initialize(page, owned.url);
    const limits = await inlineLimits(page), target = post(owned);
    const source = await owned.reply(Array.from({ length: limits.open + 1 }, (_, index) => `>>${target.id} Owned open source ${index + 1}`).join('\n'));
    await page.reload();
    const links = originalQuote(page, source, target);
    for (let index = 0; index < limits.open; index++) {
      await links.nth(index).click();
      await expect(page.locator('.inlined')).toHaveCount(index + 1);
    }
    await expect(links.nth(limits.open)).not.toHaveClass(/linkfade/);
    await links.nth(limits.open).click();
    await expect(page).toHaveURL(`${origin}${target.url}#p${target.id}`);
    await expect(page.locator('.inlined')).toHaveCount(0);
    await originalQuote(page, source, target).first().click();
    await expectInline(page, target, 'Owned inline original post');

    const chainThread = await owned.createThread('demo', 'Owned finite nesting leaf');
    const chain = [post(chainThread)];
    for (let index = 0; index <= limits.depth; index++) {
      chain.push(post(chainThread, await chainThread.reply(`>>${chain.at(-1).id}\nOwned nesting level ${index + 1}`)));
    }
    await page.goto(chainThread.url);
    let link = originalQuote(page, chain.at(-1).id, chain.at(-2));
    for (let depth = 1; depth <= limits.depth; depth++) {
      const target = chain[chain.length - depth - 1];
      await link.click();
      await expect(page.locator('.inlined')).toHaveCount(depth);
      const panel = inlineFor(page, target);
      link = panel.locator(quoteSelector(chain[chain.length - depth - 2]));
    }
    await expect(link).not.toHaveClass(/linkfade/);
    await link.click();
    await expect(page).toHaveURL(`${origin}${chainThread.url}#p${chain[0].id}`);
    await expect(page.locator('.inlined')).toHaveCount(0);
  });

  test('the pending cap keeps one active request and bounded queued placeholders before navigation cancels them', async ({ page, owned }) => {
    await initialize(page, owned.url);
    const limits = await inlineLimits(page), remote = post(await owned.createThread('test', 'Owned pending-cap target'));
    const source = await owned.reply(Array.from({ length: limits.pending + 1 }, (_, index) => `${quoteText(remote)} Owned pending source ${index + 1}`).join('\n'));
    await page.reload();
    const requests = network(page), held = await holdResponse(page, remote.path);
    try {
      const links = originalQuote(page, source, remote);
      await links.first().click();
      await held.arrived;
      for (let index = 1; index < limits.pending; index++) {
        await links.nth(index).click();
        await expect(page.locator('.inlined')).toHaveCount(index + 1);
      }
      await expect(page.locator('.inlined')).toHaveText(Array.from({ length: limits.pending }, () => 'Loading...'));
      expect(requests.map(request => new URL(request.url()).pathname)).toEqual([remote.path]);
      await links.nth(limits.pending).click();
      await expect(page).toHaveURL(`${origin}${remote.url}#p${remote.id}`);
      await held.release();
      await expect(page.locator('.inlined')).toHaveCount(0);
      expect(requests.map(request => new URL(request.url()).pathname)).toEqual([remote.path]);
    } finally { await held.release(); }
  });
});
