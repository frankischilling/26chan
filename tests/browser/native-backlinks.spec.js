import { test as base, expect } from '@playwright/test';
import { openWatcherSettings } from './helpers/watcher-settings.js';

const origin = 'http://127.0.0.1:3000';
const mobileAgent = 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36';
const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'photon', 'tomorrow'];
// Pinned theme CSS, including the v1191 extension's burichan override.
const backlinkColors = {
  yotsuba: ['rgb(0, 0, 128)', 'rgb(255, 0, 0)'],
  futaba: ['rgb(0, 0, 128)', 'rgb(255, 0, 0)'],
  'yotsuba-b': ['rgb(52, 52, 92)', 'rgb(221, 0, 0)'],
  burichan: ['rgb(52, 52, 92)', 'rgb(221, 0, 0)'],
  tomorrow: ['rgb(95, 137, 172)', 'rgb(129, 162, 190)'],
  photon: ['rgb(255, 102, 0)', 'rgb(255, 51, 0)'],
};
const trailingLines = Array.from({ length: 30 }, (_, index) => `Owned backlink trailing line ${index + 1}`).join('\n');
const absent = '9223372036854775807';

const test = base.extend({
  owned: async ({ request, context }, use) => {
    const password = 'owned-native-backlinks-password', threads = [];
    let last;
    async function write(board, resto, com, { tracked = false } = {}) {
      const client = tracked ? context.request : request;
      const response = await client.post(`/${board}/post`, {
        headers: { Origin: origin }, maxRedirects: 0,
        form: { resto, com, password, ...(resto === '0' ? { sub: 'Owned native backlinks' } : {}), ...(tracked ? { track: '1' } : {}) },
      });
      expect(response.status(), 'The real form submission must persist successfully').toBe(303);
      const location = response.headers().location;
      const ids = location?.match(/\/thread\/(\d+)#p(\d+)$/);
      expect(ids, 'The posting redirect must identify the actual thread and post').not.toBeNull();
      last = ids[2];
      return last;
    }
    async function createThread(board = 'demo', comment = 'Owned backlink original post', options) {
      const id = await write(board, '0', comment, options);
      const thread = {
        id, board, url: `/${board}/thread/${id}`, path: `/_watch/${board}/thread/${id}/posts`,
        reply: (comment, options) => write(board, id, comment, options),
      };
      threads.push(thread);
      return thread;
    }
    try {
      await use({ ...await createThread(), createThread, password,
        // This lane owns the serial test server and database. Never silently
        // accept an intervening allocation: each predictive case checks receipt IDs.
        nextId: (offset = 1) => String(BigInt(last) + BigInt(offset)),
      });
    } finally {
      for (const { board, id } of threads.reverse()) {
        expect((await request.post(`/${board}/delete`, {
          headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
        })).status(), 'Only this test\'s owned threads are deleted').toBe(303);
      }
    }
  },
});

// One browser configuration for this file permits a genuine BFCache restoration
// test; changing launch options inside a describe would require another worker.
test.use({ channel: 'chromium', launchOptions: { ignoreDefaultArgs: ['--disable-back-forward-cache'] } });

async function initialize(page, url, settings = {}, { rules, neverMobile } = {}) {
  await page.addInitScript(({ settings, rules, neverMobile }) => {
    if (localStorage.getItem('4chan-settings') === null) localStorage.setItem('4chan-settings', JSON.stringify(settings));
    if (rules !== undefined && localStorage.getItem('4chan-filters') === null) localStorage.setItem('4chan-filters', JSON.stringify(rules));
    if (neverMobile !== undefined && localStorage.getItem('4chan_never_show_mobile') === null) localStorage.setItem('4chan_never_show_mobile', neverMobile);
  }, { settings, rules, neverMobile });
  await page.goto(url);
  await expect(page.locator('#settingsWindowLink:visible, #settingsWindowLinkMobile:visible')).toBeVisible();
}

const forward = (page, source, target, board = 'demo') => page.locator(`#m${source} a.quotelink[href="/${board}/post/${target}"]`);
const backlinkRow = (page, target) => page.locator(`#bl_${target}.backlink`);
const backlink = (page, target, source, thread, board = 'demo') => backlinkRow(page, target).locator(`a.quotelink[href="/${board}/thread/${thread}#p${source}"]`);
const rule = (pattern, changes = {}) => ({ type: 2, pattern, boards: 'demo', active: true, auto: false, hide: false, color: '#ff0000', ...changes });

async function expectRows(page, target, sources, thread, board = 'demo') {
  const row = backlinkRow(page, target);
  await expect(row).toHaveCount(sources.length ? 1 : 0);
  if (sources.length) {
    await expect.poll(() => row.locator('a.quotelink').evaluateAll(nodes => nodes.map(node => node.getAttribute('href'))))
      .toEqual(sources.map(source => `/${board}/thread/${thread}#p${source}`));
  }
}

async function expectDesktopMenuOrder(page, targets, menuFirst = true) {
  await expect.poll(() => page.evaluate(ids => ids.map(id => ({
    post: id,
    controls: [...document.getElementById(`pi${id}`)?.children ?? []]
      .filter(node => node.matches('.postMenuBtn,.backlink'))
      .map(node => node.classList.contains('postMenuBtn') ? 'menu' : node.id),
  })), targets), { message: 'Desktop menu and backlink order follows the first source parsed for each target' })
    .toEqual(targets.map(id => ({ post: id,
      controls: menuFirst ? ['menu', `bl_${id}`] : [`bl_${id}`, 'menu'],
    })));
}

async function update(page, count = 1) {
  await page.locator('.threadNav.desktop a[data-cmd="update"], .threadNav.mobile a[data-cmd="update"]').filter({ visible: true }).first().click();
  await expect(page.locator('.nativeUpdaterStatus').filter({ visible: true }).first())
    .toHaveText(count ? `${count} new post${count === 1 ? '' : 's'}` : 'No new posts');
}

async function saveSettings(page, values) {
  const dialog = await openWatcherSettings(page);
  await dialog.locator('#settings-expand-all').click();
  for (const [key, value] of Object.entries(values)) await dialog.locator(`.menuOption[data-option="${key}"]`).setChecked(value);
  await Promise.all([page.waitForEvent('load'), dialog.getByRole('button', { name: 'Save Settings', exact: true }).click()]);
}

function observeFetches(page) {
  const requests = [];
  page.on('request', request => { if (['fetch', 'xhr'].includes(request.resourceType())) requests.push(new URL(request.url()).pathname); });
  return requests;
}

async function settledDOM(page) {
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

test.describe('unmodified persisted backlink graph', () => {
  test('Backlinks defaults on and its actual settings control persists across navigation', async ({ page, owned }) => {
    const reply = await owned.reply(`>>${owned.id}\nDefault backlink source`);
    await page.goto(owned.url);
    await expectRows(page, owned.id, [reply], owned.id);
    const dialog = await openWatcherSettings(page);
    await dialog.locator('#settings-expand-all').click();
    await expect(dialog.getByLabel('Backlinks', { exact: true })).toBeChecked();
    await expect(dialog).toContainText('Show who has replied to a post');
    await dialog.getByRole('button', { name: 'Close settings', exact: true }).click();
    await saveSettings(page, { backlinks: false });
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-settings')).backlinks)).toBe(false);
    await expectRows(page, owned.id, [], owned.id);
    await expect(forward(page, reply, owned.id)).toHaveText(`>>${owned.id}`);
    await page.goto('/demo/');
    await expectRows(page, owned.id, [], owned.id);
    await saveSettings(page, { backlinks: true });
    await expectRows(page, owned.id, [reply], owned.id);
    await expect(forward(page, reply, owned.id)).toHaveText(`>>${owned.id} (OP)`);
  });

  test('self and OP quotes produce one backlink per source and target in parser order', async ({ page, owned }) => {
    const predicted = owned.nextId();
    const first = await owned.reply(`>>${owned.id}\n>>${predicted}\n>>${owned.id}\n>>${predicted}\nSelf and repeated OP quotes`);
    expect(first, 'The isolated sequential form post must really quote itself').toBe(predicted);
    const second = await owned.reply(`>>${first}\n>>${owned.id}\n>>${first}\nSecond source`);
    const third = await owned.reply(`>>${owned.id}\nThird source`);
    const requests = observeFetches(page);
    await initialize(page, owned.url);
    await expectRows(page, owned.id, [first, second, third], owned.id);
    await expectRows(page, first, [first, second], owned.id);
    await expect(page.locator(`#pi${owned.id} > #bl_${owned.id}.backlink`)).toHaveCount(1);
    await expectDesktopMenuOrder(page, [owned.id, first]);
    await expect(forward(page, first, owned.id)).toHaveText([`>>${owned.id} (OP)`, `>>${owned.id} (OP)`]);
    await expect(forward(page, first, first)).toHaveText([`>>${first}`, `>>${first}`]);
    expect(requests).toEqual([]);
  });

  test('an ordinary browser form post creates a tracked source and a navigable backlink', async ({ page, owned }) => {
    await initialize(page, owned.url);
    await page.locator('#togglePostFormLink a').click();
    await page.locator('#com').fill(`>>${owned.id}\nBacklink created through the ordinary posting form`);
    await page.locator('#password').fill(owned.password);
    const action = await page.locator('form.postEditor').getAttribute('action');
    const posted = page.waitForResponse(response => response.request().method() === 'POST'
      && response.url() === new URL(action, origin).href);
    await page.getByRole('button', { name: 'Post', exact: true }).click();
    expect((await posted).status()).toBe(303);
    await expect(page).toHaveURL(new RegExp(`/demo/thread/${owned.id}#p[0-9]+$`));
    const source = page.url().match(/#p([0-9]+)$/)[1];
    await expect.poll(() => page.evaluate(({ thread, source }) => JSON.parse(localStorage.getItem(`4chan-track-demo-${thread}`) || '{}')[`>>${source}`], { thread: owned.id, source })).toBe(1);
    await expectRows(page, owned.id, [source], owned.id);
    await expect(forward(page, source, owned.id)).toHaveText(`>>${owned.id} (OP)`);
    await expect(backlink(page, owned.id, source, owned.id)).toHaveAttribute('href', `${owned.url}#p${source}`);
  });

  test('board index backlinks retain source thread routes and include a source OP', async ({ page, context, owned }) => {
    const source = await owned.createThread('demo', `>>${owned.id}\nA different thread OP replying to the target`);
    const reply = await source.reply(`>>${owned.id}\n>>${source.id}\nCross-thread index reply`);
    const requests = observeFetches(page);
    await initialize(page, '/demo/');
    await expect(page.locator(`#p${owned.id}, #p${source.id}`)).toHaveCount(2);
    // Both targets already exist. The newer thread is parsed first on the
    // board index, so numeric post order alone cannot decide menu placement.
    const parsed = [source.id, reply, owned.id].map(id => `p${id}`);
    await expect.poll(() => page.locator('.board > .thread > .postContainer > .post')
      .evaluateAll((posts, ids) => posts.map(post => post.id).filter(id => ids.includes(id)), parsed)).toEqual(parsed);
    await expectRows(page, owned.id, [source.id, reply], source.id);
    await expectRows(page, source.id, [reply], source.id);
    await expectDesktopMenuOrder(page, [owned.id], false);
    await expectDesktopMenuOrder(page, [source.id]);
    await expect(forward(page, source.id, owned.id)).toHaveText(`>>${owned.id}`);
    await expect(forward(page, reply, source.id)).toHaveText(`>>${source.id} (OP)`);
    await page.setViewportSize({ width: 480, height: 844 });
    for (const target of [owned.id, source.id]) {
      await expect(page.locator(`#p${target} > #bl_${target}.backlink.mobile`)).toHaveCount(1);
    }
    await page.setViewportSize({ width: 1280, height: 900 });
    await expectDesktopMenuOrder(page, [owned.id], false);
    await expectDesktopMenuOrder(page, [source.id]);
    const other = await context.newPage();
    try {
      await other.goto(source.url);
      await saveSettings(other, { backlinks: false });
      await expectRows(page, owned.id, [], source.id);
      await expectRows(page, source.id, [], source.id);
      await saveSettings(other, { backlinks: true });
      await expectRows(page, owned.id, [source.id, reply], source.id);
      await expectRows(page, source.id, [reply], source.id);
      await expectDesktopMenuOrder(page, [owned.id], false);
      await expectDesktopMenuOrder(page, [source.id]);
    } finally { await other.close(); }
    expect(requests).toEqual([]);
    await backlink(page, owned.id, reply, source.id).click();
    await expect(page).toHaveURL(`${origin}${source.url}#p${reply}`);
    await expect(page.locator(`#m${reply}`)).toContainText('Cross-thread index reply');
  });

  test('thread-only missing arrows follow stored same-board normalization and preserve cross-board labels without discovery fetches', async ({ page, owned }) => {
    const remote = await owned.createThread('demo', 'Remote same-board target');
    const otherBoard = await owned.createThread('test', 'Remote other-board target');
    const reply = await owned.reply(`>>${remote.id}\n>>${absent}\n>>>/demo/${remote.id}\n>>>/test/${otherBoard.id}`);
    const requests = observeFetches(page);
    await initialize(page, owned.url, { quotePreview: false });
    // The actual posting path normalizes >>>/current-board/id to >>id before
    // storage. Preserved explicit same-board labels are covered as augmented DOM.
    await expect(forward(page, reply, remote.id)).toHaveText([`>>${remote.id} →`, `>>${remote.id} →`]);
    await expect(forward(page, reply, absent)).toHaveText(`>>${absent} →`);
    await expect(forward(page, reply, otherBoard.id, 'test')).toHaveText(`>>>/test/${otherBoard.id}`);
    await expectRows(page, remote.id, [], owned.id);
    await settledDOM(page);
    expect(requests).toEqual([]);
    await page.goto('/demo/');
    await expectRows(page, remote.id, [reply], owned.id);
    await expect(forward(page, reply, absent)).toHaveText(`>>${absent}`);
    await expect(forward(page, reply, remote.id)).toHaveText([`>>${remote.id}`, `>>${remote.id}`]);
    expect(requests).toEqual([]);
  });

  test('omitted index posts do not acquire rows and missing index quotes do not gain arrows', async ({ page, owned }) => {
    const omitted = await owned.reply('Omitted target');
    for (let i = 0; i < 3; i++) await owned.reply(`Newer visible reply ${i}`);
    const reply = await owned.reply(`>>${omitted}\n>>${owned.id}\nVisible index source`);
    const requests = observeFetches(page);
    await initialize(page, '/demo/');
    await expect(page.locator(`#t${owned.id} .omitted`)).toBeVisible();
    await expect(page.locator(`#p${omitted}`)).toHaveCount(0);
    await expectRows(page, omitted, [], owned.id);
    await expectRows(page, owned.id, [reply], owned.id);
    await expect(forward(page, reply, omitted)).toHaveText(`>>${omitted}`);
    expect(requests).toEqual([]);
    await page.locator(`#t${owned.id} .omitted a`).click();
    await expectRows(page, omitted, [reply], owned.id);
  });

  test('updater rows and annotations are complete at the actual completion event without replacing original anchors', async ({ page, owned }) => {
    const original = await owned.reply(`>>${owned.id}\nOriginal source`);
    await initialize(page, owned.url);
    await expectRows(page, owned.id, [original], owned.id);
    await forward(page, original, owned.id).evaluate(node => { window.ownedOriginalQuote = node; });
    const added = await owned.reply(`>>${owned.id}\n>>${original}\nNew updater source`);
    await page.evaluate(({ op, added }) => {
      window.backlinksAtCompletion = null;
      document.addEventListener('4chanThreadUpdated', () => {
        window.backlinksAtCompletion = {
          sources: [...document.querySelectorAll(`#bl_${op} a.quotelink`)].map(node => node.getAttribute('href')),
          text: document.querySelector(`#m${added} a.quotelink`).textContent,
        };
      }, { once: true });
    }, { op: owned.id, added });
    await update(page);
    expect(await page.evaluate(() => window.backlinksAtCompletion)).toEqual({
      sources: [original, added].map(id => `/demo/thread/${owned.id}#p${id}`), text: `>>${owned.id} (OP)`,
    });
    await expectRows(page, original, [added], owned.id);
    await expectDesktopMenuOrder(page, [owned.id, original]);
    expect(await forward(page, original, owned.id).evaluate(node => node === window.ownedOriginalQuote)).toBe(true);
    await expect(forward(page, original, owned.id)).toHaveAttribute('href', `/demo/post/${owned.id}`);
    await page.setViewportSize({ width: 1270, height: 900 });
    await page.setViewportSize({ width: 1280, height: 900 });
    await expectRows(page, owned.id, [original, added], owned.id);
    await expect(forward(page, original, owned.id)).toHaveText(`>>${owned.id} (OP)`);
  });

  test('a formerly absent target appended later is not retrospectively linked until full navigation', async ({ page, owned }) => {
    await initialize(page, owned.url);
    const sourceId = owned.nextId(), futureId = owned.nextId(2);
    const source = await owned.reply(`>>${futureId}\nThis target is absent when this source is first parsed`);
    expect(source).toBe(sourceId);
    await update(page);
    await expect(forward(page, source, futureId)).toHaveText(`>>${futureId} →`);
    const target = await owned.reply('The formerly absent target is now persisted');
    expect(target, 'The later persisted post must be the exact quoted ID').toBe(futureId);
    await page.waitForTimeout(1050);
    await update(page);
    await expect(page.locator(`#p${target}`)).toBeVisible();
    await expectRows(page, target, [], owned.id);
    await expect(forward(page, source, target)).toHaveText(`>>${target} →`);
    const fresh = await owned.reply(`>>${target}\nA fresh source can now resolve this live target`);
    await page.waitForTimeout(1050);
    await update(page);
    await expectRows(page, target, [fresh], owned.id);
    await page.reload();
    await expectRows(page, target, [source, fresh], owned.id);
    await expect(forward(page, source, target)).toHaveText(`>>${target}`);
  });

  test('real posting receipts compose You before OP and survive live backlink toggles', async ({ page, context, owned }) => {
    const tracked = await owned.createThread('demo', 'Actual tracked original post', { tracked: true });
    const reply = await tracked.reply(`>>${tracked.id}\nReply to the tracked OP`);
    expect((await context.cookies()).some(cookie => cookie.name === `board-posted-${tracked.id}`)).toBe(true);
    await initialize(page, tracked.url);
    await expect.poll(() => page.evaluate(id => JSON.parse(localStorage.getItem(`4chan-track-demo-${id}`) || '{}')[`>>${id}`], tracked.id)).toBe(1);
    const link = forward(page, reply, tracked.id);
    await expect(link).toHaveText(`>>${tracked.id} (You) (OP)`);
    await expect(link).toHaveClass(/ql-tracked/);
    await link.evaluate(node => { window.ownedTrackedQuote = node; });
    const other = await context.newPage();
    try {
      await other.goto(tracked.url);
      await saveSettings(other, { backlinks: false });
      await expect(link).toHaveText(`>>${tracked.id} (You)`);
      await expect(link).toHaveClass(/ql-tracked/);
      await saveSettings(other, { backlinks: true });
      await expect(link).toHaveText(`>>${tracked.id} (You) (OP)`);
      await expectRows(page, tracked.id, [reply], tracked.id);
      expect(await link.evaluate(node => node === window.ownedTrackedQuote)).toBe(true);
      await expect(link).toHaveAttribute('href', `/demo/post/${tracked.id}`);
    } finally { await other.close(); }
    expect((await context.cookies()).some(cookie => cookie.name === `board-posted-${tracked.id}`)).toBe(false);
  });

  test('new OP and missing-target annotations cannot become filter input, while literal text remains matchable', async ({ page, owned }) => {
    const opQuote = await owned.reply(`>>${owned.id}\nNo literal annotation here`);
    const missingQuote = await owned.reply(`>>${absent}\nNo literal arrow here`);
    const literal = await owned.reply('Literal annotation control (OP) →');
    await initialize(page, owned.url, { filter: true }, { rules: [rule('/\\(OP\\)|→/')] });
    await expect(forward(page, opQuote, owned.id)).toHaveText(`>>${owned.id} (OP)`);
    await expect(forward(page, missingQuote, absent)).toHaveText(`>>${absent} →`);
    await expect(page.locator(`#p${literal}`)).toHaveClass(/filter-hl/);
    await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
    await expect(page.locator(`#p${opQuote}, #p${missingQuote}`).filter({ has: page.locator('.postMessage') })).toHaveCount(2);
    await expect(page.locator(`#p${opQuote}`)).not.toHaveClass(/filter-hl|post-hidden/);
    await expect(page.locator(`#p${missingQuote}`)).not.toHaveClass(/filter-hl|post-hidden/);
  });

  test('a source hidden by the actual filter still contributes its backlink and hiding the target hides the row', async ({ page, owned }) => {
    const target = await owned.reply('Visible backlink target');
    const source = await owned.reply(`>>${target}\nHide this backlink source`);
    await initialize(page, owned.url, { filter: true }, { rules: [rule('Hide this backlink source', { hide: true })] });
    await expect(page.locator(`#p${source}`)).toHaveClass(/post-hidden/);
    await expect(page.locator(`#m${source}`)).toBeHidden();
    await expectRows(page, target, [source], owned.id);
    await expect(backlinkRow(page, target)).toBeVisible();
    await page.getByRole('button', { name: `Post menu for post ${target}`, exact: true }).click();
    await page.getByRole('menuitem', { name: 'Hide post', exact: true }).click();
    await expect(page.locator(`#pc${target}`)).toHaveClass(/post-hidden/);
    await expect(backlinkRow(page, target)).toBeHidden();
    await expectRows(page, target, [source], owned.id);
  });

  test('real cross-tab setting saves disable and restore owned rows and suffixes without replacing the document', async ({ page, context, owned }) => {
    const source = await owned.reply(`>>${owned.id}\n>>${absent}\nSetting changes`);
    await initialize(page, owned.url);
    await expectRows(page, owned.id, [source], owned.id);
    await expectDesktopMenuOrder(page, [owned.id]);
    await page.evaluate(() => { window.ownedBacklinkDocument = document; });
    await forward(page, source, owned.id).evaluate(node => { window.ownedBacklinkAnchor = node; });
    const other = await context.newPage(), requests = observeFetches(page);
    try {
      await other.goto(owned.url);
      for (const key of ['backlinks', 'disableAll']) {
        await saveSettings(other, { [key]: key === 'disableAll' });
        await expectRows(page, owned.id, [], owned.id);
        await expect(forward(page, source, owned.id)).toHaveText(`>>${owned.id}`);
        await expect(forward(page, source, absent)).toHaveText(`>>${absent}`);
        await saveSettings(other, { [key]: key !== 'disableAll' });
        await expectRows(page, owned.id, [source], owned.id);
        await expectDesktopMenuOrder(page, [owned.id]);
        await expect(forward(page, source, owned.id)).toHaveText(`>>${owned.id} (OP)`);
        await expect(forward(page, source, absent)).toHaveText(`>>${absent} →`);
      }
      expect(await page.evaluate(() => window.ownedBacklinkDocument === document)).toBe(true);
      expect(await forward(page, source, owned.id).evaluate(node => node === window.ownedBacklinkAnchor)).toBe(true);
      expect(requests).toEqual([]);
    } finally { await other.close(); }
  });

  test('no-JavaScript forward navigation matches the generated canonical backlink destination', async ({ page, browser, owned }) => {
    const source = await owned.reply(`>>${owned.id}\nNo-JavaScript navigation control`);
    await initialize(page, owned.url, { quotePreview: false });
    await expectRows(page, owned.id, [source], owned.id);
    await backlink(page, owned.id, source, owned.id).click();
    await expect(page).toHaveURL(`${origin}${owned.url}#p${source}`);
    const context = await browser.newContext({ javaScriptEnabled: false });
    try {
      const plain = await context.newPage();
      await plain.goto(`${origin}${owned.url}`);
      await expect(plain.locator('.backlink')).toHaveCount(0);
      await expect(forward(plain, source, owned.id)).toHaveText(`>>${owned.id}`);
      await forward(plain, source, owned.id).click();
      await expect(plain).toHaveURL(`${origin}${owned.url}#p${owned.id}`);
      await expect(plain.locator(`#p${owned.id} .postNum`)).toHaveAttribute('href', `${owned.url}#p${owned.id}`);
    } finally { await context.close(); }
  });
});

test.describe('persisted layout and preview integration', () => {
  for (const scenario of [
    { name: 'desktop device at the mobile boundary', width: 480, mobileRow: true },
    { name: 'desktop device above the mobile boundary', width: 481, mobileRow: false },
    { name: 'mobile device in a wide viewport', width: 1280, device: true, mobileRow: false },
    { name: 'narrow viewport with never-mobile enabled', width: 390, neverMobile: 'true', mobileRow: false },
  ]) {
    test(`${scenario.name} uses layout placement and preserves navigation with quote preview disabled`, async ({ browser, owned }) => {
      const target = await owned.reply('Reply target for layout navigation');
      const source = await owned.reply(`>>${target}\nLayout navigation source`);
      const context = await browser.newContext({ viewport: { width: scenario.width, height: 844 },
        ...(scenario.device ? { userAgent: mobileAgent, isMobile: true, hasTouch: true } : {}),
      });
      try {
        const page = await context.newPage();
        await initialize(page, `${origin}${owned.url}`, { quotePreview: false }, { neverMobile: scenario.neverMobile });
        await expectRows(page, target, [source], owned.id);
        const row = backlinkRow(page, target);
        expect(await row.evaluate(node => ({ parent: node.parentElement.id, mobile: node.classList.contains('mobile') })))
          .toEqual({ parent: `${scenario.mobileRow ? 'p' : 'pi'}${target}`, mobile: scenario.mobileRow });
        await expect(row.locator('a.quoteLink')).toHaveCount(scenario.mobileRow ? 1 : 0);
        const navigation = scenario.mobileRow ? row.locator('a.quoteLink') : row.locator('a.quotelink');
        await expect(navigation).toHaveAttribute('href', `${owned.url}#p${source}`);
        const requests = observeFetches(page);
        await navigation.click();
        await expect(page).toHaveURL(`${origin}${owned.url}#p${source}`);
        await expect(page.locator('#quote-preview')).toHaveCount(0);
        expect(requests).toEqual([]);
      } finally { await context.close(); }
    });
  }

  test('viewport and actual never-mobile storage changes move one row without duplicating sources', async ({ page, context, owned }) => {
    const source = await owned.reply(`>>${owned.id}\nResponsive source`);
    await initialize(page, owned.url, { quotePreview: false });
    await expectRows(page, owned.id, [source], owned.id);
    await expectDesktopMenuOrder(page, [owned.id]);
    await forward(page, source, owned.id).evaluate(node => { window.ownedResponsiveQuote = node; });
    const other = await context.newPage();
    try {
      await other.goto(owned.url);
      await page.setViewportSize({ width: 480, height: 844 });
      await expect(page.locator(`#p${owned.id} > #bl_${owned.id}.backlink.mobile`)).toHaveCount(1);
      await expect(backlinkRow(page, owned.id)).toBeHidden();
      await expect(backlinkRow(page, owned.id).locator('a.quoteLink')).toHaveCount(1);
      await other.evaluate(() => localStorage.setItem('4chan_never_show_mobile', 'true'));
      await expect(page.locator(`#pi${owned.id} > #bl_${owned.id}`)).toHaveCount(1);
      await expect(backlinkRow(page, owned.id)).toBeVisible();
      await expectDesktopMenuOrder(page, [owned.id]);
      await expect(backlinkRow(page, owned.id).locator('a.quoteLink')).toHaveCount(0);
      await other.evaluate(() => localStorage.setItem('4chan_never_show_mobile', 'false'));
      await expect(page.locator(`#p${owned.id} > #bl_${owned.id}.mobile`)).toHaveCount(1);
      await page.setViewportSize({ width: 481, height: 844 });
      await expect(page.locator(`#pi${owned.id} > #bl_${owned.id}`)).toHaveCount(1);
      await expectRows(page, owned.id, [source], owned.id);
      await expectDesktopMenuOrder(page, [owned.id]);
      expect(await forward(page, source, owned.id).evaluate(node => node === window.ownedResponsiveQuote)).toBe(true);
    } finally { await other.close(); }
  });

  test('a mobile device retains exactly one backlink # while preview toggles add and remove only forward companions', async ({ browser, owned }) => {
    const target = await owned.reply('Mobile reply target');
    const source = await owned.reply(`>>${target}\nMobile ownership composition`);
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, userAgent: mobileAgent, isMobile: true, hasTouch: true });
    try {
      const page = await context.newPage(), other = await context.newPage();
      await initialize(page, `${origin}${owned.url}`);
      await other.goto(`${origin}${owned.url}`);
      await expectRows(page, target, [source], owned.id);
      for (const enabled of [false, true, false, true]) {
        await saveSettings(other, { quotePreview: enabled });
        await expect(backlinkRow(page, target).locator('a.quoteLink')).toHaveCount(1);
        await expect(backlinkRow(page, target).locator('a.quoteLink')).toHaveAttribute('href', `${owned.url}#p${source}`);
        await expect(page.locator(`#m${source} a.quoteLink`)).toHaveCount(enabled ? 1 : 0);
        await expectRows(page, target, [source], owned.id);
      }
      await backlinkRow(page, target).locator('a.quoteLink').tap();
      await expect(page).toHaveURL(`${origin}${owned.url}#p${source}`);
    } finally { await context.close(); }
  });

  test('receipt You text remains filterable after adding the owned OP annotation', async ({ page, owned }) => {
    const tracked = await owned.createThread('demo', 'Tracked filter control', { tracked: true });
    const source = await tracked.reply(`>>${tracked.id}\nTracked annotation filter input`);
    await initialize(page, tracked.url, { filter: true }, { rules: [rule('/\\(You\\)/')] });
    await expect(forward(page, source, tracked.id)).toHaveText(`>>${tracked.id} (You) (OP)`);
    await expect(page.locator(`#p${source}`)).toHaveClass(/filter-hl/);
    await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
  });

  test('existing mobile forward # text remains filterable without treating the target backlink row as comment input', async ({ browser, owned }) => {
    const target = await owned.reply('Target without hash text');
    const source = await owned.reply(`>>${target}\nMobile companion filter control`);
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, userAgent: mobileAgent, isMobile: true, hasTouch: true });
    try {
      const page = await context.newPage();
      await initialize(page, `${origin}${owned.url}`, { filter: true }, { rules: [rule('/#/')] });
      await expect(page.locator(`#m${source} a.quoteLink`)).toHaveCount(1);
      await expectRows(page, target, [source], owned.id);
      await expect(page.locator(`#p${source}`)).toHaveClass(/filter-hl/);
      await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
      await expect(page.locator(`#p${target}`)).not.toHaveClass(/filter-hl|post-hidden/);
    } finally { await context.close(); }
  });

  for (const mobile of [false, true]) {
    test(`local ${mobile ? 'mobile' : 'desktop'} previews copy controller backlinks without IDs or graph side effects`, async ({ browser, owned }) => {
      const target = await owned.reply('Owned preview target with incoming replies');
      const first = await owned.reply(`>>${target}\nFirst incoming source`);
      const second = await owned.reply(`>>${target}\n${trailingLines}`);
      const context = await browser.newContext({ viewport: { width: mobile ? 390 : 1280, height: 440 },
        ...(mobile ? { userAgent: mobileAgent, isMobile: true, hasTouch: true } : {}),
      });
      try {
        const page = await context.newPage();
        await initialize(page, `${origin}${owned.url}`);
        await expectRows(page, target, [first, second], owned.id);
        const link = forward(page, second, target);
        await link.evaluate(node => node.scrollIntoView({ block: 'start' }));
        await page.evaluate(() => scrollBy(0, -40));
        const requests = observeFetches(page);
        if (mobile) await link.tap(); else await link.hover();
        const preview = page.locator('#quote-preview');
        await expect(preview).toBeVisible();
        await expect(preview.locator('.postMessage')).toHaveText('Owned preview target with incoming replies');
        await expect(preview.locator('.backlink')).toHaveCount(1);
        expect(await preview.locator('.backlink a.quotelink').evaluateAll(nodes => nodes.map(node => node.getAttribute('href'))))
          .toEqual([first, second].map(id => `${owned.url}#p${id}`));
        if (mobile) await expect(preview.locator('.backlink.mobile')).toBeHidden();
        else await expect(preview.locator('.backlink')).toBeVisible();
        await expect(preview.locator('[id], form, input, button, script, iframe')).toHaveCount(0);
        await expectRows(page, target, [first, second], owned.id);
        expect(await page.locator('[id]').evaluateAll(nodes => nodes.length === new Set(nodes.map(node => node.id)).size)).toBe(true);
        expect(requests).toEqual([]);
      } finally { await context.close(); }
    });
  }

  test('a genuine remote preview containing a quote cannot register a new backlink on the live page', async ({ page, owned }) => {
    const remote = await owned.createThread('demo', `>>${owned.id}\nA remote post must not join this page's graph`);
    const healthy = await owned.reply(`>>${owned.id}\nHealthy local graph source`);
    const source = await owned.reply(`>>${remote.id}\nPreview the remote source`);
    await initialize(page, owned.url);
    await expectRows(page, owned.id, [healthy], owned.id);
    const requests = observeFetches(page);
    await forward(page, source, remote.id).hover();
    await expect(page.locator('#quote-preview .postMessage')).toContainText("A remote post must not join this page's graph");
    await expect(page.locator('#quote-preview .backlink')).toHaveCount(0);
    await settledDOM(page);
    await expectRows(page, owned.id, [healthy], owned.id);
    expect(requests).toEqual([`/_watch/demo/post/${remote.id}`]);
  });

  for (const variant of ['exact repeated label', 'single direct quote', 'OP annotation', 'You annotation']) {
    test(`a backlink-triggered local preview applies the exact direct-quote dotted rule for ${variant}`, async ({ page, owned }) => {
      const owner = variant === 'OP annotation' ? owned.id
        : await owned.reply('Owner of the preview-triggering backlink', { tracked: variant === 'You annotation' });
      const other = await owned.reply('Another direct quote target');
      await owned.reply(trailingLines);
      const text = variant === 'single direct quote' ? `>>${owner}`
        : variant === 'exact repeated label' ? `>>${owner}\n>>${other}\n>>${owner}` : `>>${owner}\n>>${other}`;
      const source = await owned.reply(`${text}\nSource below the viewport`);
      await page.setViewportSize({ width: 1280, height: 440 });
      await initialize(page, owned.url);
      await expectRows(page, owner, [source], owned.id);
      if (variant === 'You annotation') await expect(forward(page, source, owner)).toHaveText(`>>${owner} (You)`);
      const link = backlink(page, owner, source, owned.id);
      await link.evaluate(node => node.scrollIntoView({ block: 'start' }));
      await page.evaluate(() => scrollBy(0, -40));
      const requests = observeFetches(page);
      await link.hover();
      const preview = page.locator('#quote-preview');
      await expect(preview).toBeVisible();
      await expect(preview.locator('.postMessage')).toContainText('Source below the viewport');
      await expect(preview.locator('.postMessage .dotted')).toHaveCount(variant === 'exact repeated label' ? 1 : 0);
      if (variant === 'exact repeated label') {
        await expect(preview.locator('.postMessage > a.quotelink').first()).toHaveClass(/dotted/);
        await expect(preview.locator('.postMessage > a.quotelink').last()).not.toHaveClass(/dotted/);
      }
      expect(requests).toEqual([]);
    });
  }

  for (const theme of themes) {
    test(`backlink rows remain readable and inside the post in ${theme} at both layouts`, async ({ page, context, owned }) => {
      const target = await owned.reply('Themed backlink target');
      const first = await owned.reply(`>>${target}\nFirst themed source`);
      const second = await owned.reply(`>>${target}\nSecond themed source`);
      await context.addCookies([{ name: 'board-theme-ws', value: theme, url: origin, httpOnly: true, sameSite: 'Lax' }]);
      await initialize(page, owned.url, { quotePreview: false });
      for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 900 });
        await expectRows(page, target, [first, second], owned.id);
        const row = backlinkRow(page, target);
        await expect(row).toBeVisible();
        const parent = page.locator(`#p${target}`), bounds = await row.boundingBox(), parentBounds = await parent.boundingBox();
        expect(bounds.width).toBeGreaterThan(0);
        expect(bounds.height).toBeGreaterThan(0);
        expect(bounds.x).toBeGreaterThanOrEqual(parentBounds.x - 1);
        expect(bounds.x + bounds.width).toBeLessThanOrEqual(parentBounds.x + parentBounds.width + 1);
        expect(bounds.x + bounds.width).toBeLessThanOrEqual(width + 1);
        const firstLink = row.locator('a.quotelink').first();
        await expect(firstLink).toHaveCSS('color', backlinkColors[theme][0]);
        await expect(firstLink).toHaveCSS('text-decoration-line', 'underline');
        await firstLink.hover();
        await expect(firstLink).toHaveCSS('color', backlinkColors[theme][1]);
        await page.mouse.move(0, 0);
        expect(await firstLink.evaluate(node => Number.parseFloat(getComputedStyle(node).fontSize))).toBeGreaterThanOrEqual(10);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      }
    });
  }
});

test('the real backlink module is allowed as a same-origin page asset but rejected as a worker and foreign-origin module', async ({ page, request, owned }) => {
  const source = await owned.reply(`>>${owned.id}\nCSP backlink control`);
  const path = '/static/native-backlinks.v1.js';
  const response = await request.get(path), foreign = await request.get(`http://localhost:3000${path}`);
  expect(response.status()).toBe(200);
  expect(foreign.status()).toBe(200);
  expect(response.headers()['content-type']).toMatch(/javascript/);
  const bytes = await response.body();
  expect(bytes.byteLength).toBeLessThanOrEqual(32768);
  expect(await foreign.body()).toEqual(bytes);
  await initialize(page, owned.url);
  await expectRows(page, owned.id, [source], owned.id);
  const result = await page.evaluate(async path => {
    const violations = [];
    const receive = event => violations.push({ directive: event.effectiveDirective, blocked: event.blockedURI });
    document.addEventListener('securitypolicyviolation', receive);
    let allowed;
    try {
      allowed = await new Promise(resolve => {
        const worker = new Worker('/static/native-filter.v1.js', { type: 'module' });
        const timer = setTimeout(() => { worker.terminate(); resolve('timeout'); }, 5000);
        worker.onerror = () => { clearTimeout(timer); worker.terminate(); resolve('error'); };
        worker.onmessage = event => { clearTimeout(timer); worker.terminate(); resolve(JSON.parse(event.data).status); };
        worker.postMessage(JSON.stringify({ version: 1, board: 'demo', filters: [], posts: [], mode: 'page', thread: true }));
      });
      let rejectedWorker;
      try { rejectedWorker = new Worker(path, { type: 'module' }); rejectedWorker.onerror = event => event.preventDefault(); }
      catch { /* Synchronous CSP rejection also emits a policy violation. */ }
      let foreignRejected = false;
      try { await import(`http://localhost:3000${path}`); }
      catch { foreignRejected = true; }
      for (let i = 0; i < 50 && violations.length < 2; i++) await new Promise(resolve => setTimeout(resolve, 20));
      rejectedWorker?.terminate();
      return { allowed, foreignRejected, violations };
    } finally { document.removeEventListener('securitypolicyviolation', receive); }
  }, path);
  expect(result.allowed).toBe('ok');
  expect(result.foreignRejected).toBe(true);
  expect(result.violations).toEqual(expect.arrayContaining([
    { directive: 'worker-src', blocked: `${origin}${path}` },
    expect.objectContaining({ directive: expect.stringMatching(/^script-src/), blocked: `http://localhost:3000${path}` }),
  ]));
  await expectRows(page, owned.id, [source], owned.id);
});

async function appendFixturePosts(page, thread, definitions) {
  await page.evaluate(({ thread, definitions }) => {
    const fragment = document.createDocumentFragment();
    for (const definition of definitions) {
      const article = document.createElement('article');
      article.id = `pc${definition.id}`; article.className = 'postContainer replyContainer';
      article.dataset.ownedBacklinkFixture = 'true';
      const post = document.createElement('div'); post.id = `p${definition.id}`; post.className = 'post reply';
      const info = document.createElement('div'); info.id = `pi${definition.id}`; info.className = 'postInfo';
      const name = document.createElement('span'); name.className = 'name'; name.textContent = 'Owned augmented fixture'; info.append(name);
      const message = document.createElement('blockquote'); message.id = `m${definition.id}`; message.className = 'postMessage';
      let parent = message;
      for (let depth = 0; depth < (definition.depth ?? 0); depth++) {
        const span = document.createElement('span'); parent.append(span); parent = span;
      }
      for (const quote of definition.quotes ?? []) {
        const anchor = document.createElement('a'); anchor.className = 'quotelink';
        anchor.setAttribute('href', quote.href); anchor.textContent = quote.text; parent.append(anchor, ' ');
      }
      if (definition.text) message.append(definition.text);
      for (let pair = 0; pair < (definition.nodePairs ?? 0); pair++) message.append('x', document.createElement('wbr'));
      post.append(info, message); article.append(post); fragment.append(article);
    }
    document.getElementById(`t${thread}`).append(fragment);
  }, { thread, definitions });
}

async function activeLimits(page) {
  // Read the controller's published finite contract. This avoids coupling the
  // browser fixture to an intermediate implementation's private markers.
  const limits = await page.evaluate(async () => (await import('/static/native-backlinks.v1.js')).BACKLINK_LIMITS);
  for (const key of ['linksPerPost', 'links', 'edges', 'nodes', 'depth', 'html', 'text']) {
    expect(Number.isInteger(limits[key]) && limits[key] > 0, `Missing finite ${key} contract`).toBe(true);
  }
  return limits;
}

test.describe('explicitly augmented DOM and graph bounds', () => {
  test('a nested owner quote does not acquire the direct-quote dotted cue even with two other direct quotes', async ({ page, owned }) => {
    const owner = await owned.reply('Nested-quote backlink owner');
    const other = await owned.reply('Other direct quote target');
    await owned.reply(trailingLines);
    const source = await owned.reply(`>>${owner}\n>>${other}\n>>${other}\nNested cue source`);
    await page.setViewportSize({ width: 1280, height: 440 });
    await initialize(page, owned.url);
    await expectRows(page, owner, [source], owned.id);
    await forward(page, source, owner).evaluate(node => {
      const wrapper = document.createElement('span'); node.before(wrapper); wrapper.append(node);
    });
    const trigger = backlink(page, owner, source, owned.id);
    await trigger.evaluate(node => node.scrollIntoView({ block: 'start' }));
    await page.evaluate(() => scrollBy(0, -40));
    const requests = observeFetches(page);
    await trigger.hover();
    await expect(page.locator('#quote-preview .postMessage')).toContainText('Nested cue source');
    await expect(page.locator('#quote-preview .postMessage > a.quotelink')).toHaveCount(2);
    await expect(page.locator('#quote-preview .postMessage .dotted')).toHaveCount(0);
    expect(requests).toEqual([]);
  });

  test('cross-board ID collisions and mismatched known-thread routes cannot create backlinks', async ({ page, owned }) => {
    const target = await owned.reply('Canonical target for augmented route checks');
    const healthy = await owned.reply(`>>${target}\nHealthy persisted route control`);
    await initialize(page, owned.url, { quotePreview: false });
    await expectRows(page, target, [healthy], owned.id);
    const requests = observeFetches(page), baseId = 900000000000000000n;
    const invalid = [
      `/test/post/${target}`, `/test/thread/${owned.id}#p${target}`,
      `/demo/thread/${String(BigInt(owned.id) + 1n)}#p${target}`,
      `http://localhost:3000/demo/thread/${owned.id}#p${target}`,
      `/demo/post/${target}?graph=forged`, `#p0${target}`,
    ].map((href, index) => ({ id: String(baseId + BigInt(index)), quotes: [{ href, text: `>>${target}` }] }));
    const canonical = String(baseId + 20n), explicit = String(baseId + 21n);
    await appendFixturePosts(page, owned.id, [...invalid,
      { id: canonical, quotes: [{ href: `/demo/thread/${owned.id}#p${target}`, text: `>>${target}` }] },
      { id: explicit, quotes: [{ href: `/demo/post/${absent}`, text: `>>>/demo/${absent}` }] },
    ]);
    await expectRows(page, target, [healthy, canonical], owned.id);
    await expect(forward(page, explicit, absent)).toHaveText(`>>>/demo/${absent}`);
    await expectRows(page, absent, [], owned.id);
    for (const { id, quotes } of invalid) await expect(page.locator(`#m${id} a`)).toHaveAttribute('href', quotes[0].href);
    await settledDOM(page);
    expect(requests).toEqual([]);
  });

  test('foreign backlink-shaped rows are preserved on disable and excluded from trusted local preview copies', async ({ page, context, owned }) => {
    const target = await owned.reply('Preview target with a trusted controller row');
    const source = await owned.reply(`>>${target}\n${trailingLines}`);
    await page.setViewportSize({ width: 1280, height: 440 });
    await initialize(page, owned.url);
    await expectRows(page, target, [source], owned.id);
    await page.locator(`#pi${target}`).evaluate((info, target) => {
      const row = document.createElement('div'); row.className = 'backlink'; row.id = 'owned-foreign-backlink';
      const anchor = document.createElement('a'); anchor.className = 'quotelink';
      anchor.setAttribute('href', `/demo/post/${target}`); anchor.textContent = 'FORGED_BACKLINK_ROW';
      row.append(anchor); info.append(row); window.ownedForeignBacklink = row;
    }, target);
    const link = forward(page, source, target), requests = observeFetches(page);
    await link.evaluate(node => node.scrollIntoView({ block: 'start' }));
    await page.evaluate(() => scrollBy(0, -40));
    await link.hover();
    await expect(page.locator('#quote-preview')).toBeVisible();
    await expect(page.locator('#quote-preview .backlink a.quotelink')).toHaveCount(1);
    await expect(page.locator('#quote-preview')).not.toContainText('FORGED_BACKLINK_ROW');
    await expectRows(page, target, [source], owned.id);
    const other = await context.newPage();
    try {
      await other.goto(owned.url);
      await saveSettings(other, { backlinks: false });
      await expectRows(page, target, [], owned.id);
      await expect(page.locator('#owned-foreign-backlink')).toContainText('FORGED_BACKLINK_ROW');
      expect(await page.evaluate(() => document.getElementById('owned-foreign-backlink') === window.ownedForeignBacklink)).toBe(true);
      expect(requests).toEqual([]);
    } finally { await other.close(); }
  });

  test('a colliding unowned bl_ID is neither overwritten nor trusted and an independent target still works', async ({ page, context, owned }) => {
    const target = await owned.reply('Independent collision control target');
    const source = await owned.reply(`>>${owned.id}\n>>${target}\nCollision source`);
    await initialize(page, owned.url, { backlinks: false });
    await page.locator(`#pi${owned.id}`).evaluate((info, id) => {
      const row = document.createElement('div'); row.id = `bl_${id}`; row.className = 'backlink';
      row.textContent = 'UNOWNED_ID_COLLISION'; info.append(row); window.collidingBacklink = row;
    }, owned.id);
    const other = await context.newPage();
    try {
      await other.goto(owned.url);
      await saveSettings(other, { backlinks: true });
      await expectRows(page, target, [source], owned.id);
      await expect(backlinkRow(page, owned.id)).toHaveCount(1);
      await expect(backlinkRow(page, owned.id)).toHaveText('UNOWNED_ID_COLLISION');
      expect(await backlinkRow(page, owned.id).evaluate(node => node === window.collidingBacklink)).toBe(true);
      await saveSettings(other, { disableAll: true });
      await expect(backlinkRow(page, owned.id)).toHaveText('UNOWNED_ID_COLLISION');
      await expectRows(page, target, [], owned.id);
    } finally { await other.close(); }
  });

  test('backlink-like classes on ordinary message text cannot impersonate filter-excluded annotation ownership', async ({ page, owned }) => {
    const target = await owned.reply('Augmented filter target');
    const source = await owned.reply(`>>${owned.id}\nHealthy owned annotation`);
    await initialize(page, owned.url, { filter: true }, { rules: [rule('/FORGED_ANNOTATION/')] });
    await expectRows(page, owned.id, [source], owned.id);
    await page.locator(`#m${target}`).evaluate(message => {
      const span = document.createElement('span');
      span.className = 'backlink quoteLink native-backlink-annotation'; span.textContent = 'FORGED_ANNOTATION'; message.append(span);
    });
    await expect(page.locator(`#p${target}`)).toHaveClass(/filter-hl/);
    await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
    await expect(page.locator(`#p${source}`)).not.toHaveClass(/filter-hl|post-hidden/);
  });

  for (const defect of ['depth', 'HTML', 'text', 'nodes', 'links per source', 'annotation expansion']) {
    test(`${defect} overflow leaves the complete source unchanged and an independent healthy source can register`, async ({ page, owned }) => {
      const persisted = await owned.reply(`>>${owned.id}\nHealthy persisted bound control`);
      await initialize(page, owned.url, { quotePreview: false });
      await expectRows(page, owned.id, [persisted], owned.id);
      const limits = await activeLimits(page), bad = '900000000000000001', healthy = '900000000000000002';
      const quote = { href: `/demo/post/${owned.id}`, text: `>>${owned.id}` };
      const definition = { id: bad, quotes: [quote] };
      if (defect === 'depth') definition.depth = limits.depth + 1;
      if (defect === 'HTML') definition.text = '&'.repeat(Math.floor(limits.html / 5) + 1);
      if (defect === 'text') definition.text = 'x'.repeat(limits.text + 1);
      if (defect === 'nodes') definition.nodePairs = Math.ceil(limits.nodes / 2);
      if (defect === 'links per source') definition.quotes = Array.from({ length: limits.linksPerPost + 1 }, () => quote);
      if (defect === 'annotation expansion') definition.text = 'x'.repeat(limits.text - quote.text.length - 3);
      const requests = observeFetches(page);
      await appendFixturePosts(page, owned.id, [definition, { id: healthy, quotes: [quote] }]);
      await expectRows(page, owned.id, [persisted, healthy], owned.id);
      await expect(page.locator(`#m${bad} a.quotelink`).first()).toHaveText(quote.text);
      await expect(page.locator(`#m${bad} a.quotelink`).first()).toHaveAttribute('href', quote.href);
      await expect(page.locator(`#m${bad}`)).not.toContainText('(OP)');
      await expect(page.locator(`#m${healthy} a.quotelink`)).toHaveText(`${quote.text} (OP)`);
      expect(requests).toEqual([]);
      // A full navigation is an independent healthy control, without granting a
      // rejected source a retrospective rescan in the original document.
      await page.reload();
      await expectRows(page, owned.id, [persisted], owned.id);
    });
  }

  test('the finite graph edge budget rejects an entire overflowing source and remains recoverable', async ({ page, owned }) => {
    const persisted = await owned.reply(`>>${owned.id}\nOne real edge before the augmented graph`);
    await initialize(page, owned.url, { quotePreview: false });
    await expectRows(page, owned.id, [persisted], owned.id);
    const limits = await activeLimits(page), start = 900000000000000000n;
    const width = Math.min(limits.linksPerPost, 128);
    const targets = Array.from({ length: width }, (_, i) => String(start + BigInt(i)));
    const definitions = targets.map(id => ({ id, text: 'Augmented graph target' }));
    const admitted = [];
    let remaining = limits.edges - 1;
    for (let index = 0; remaining > 0; index++) {
      const count = Math.min(width, remaining), id = String(start + 1000n + BigInt(index));
      admitted.push(id);
      definitions.push({ id, quotes: targets.slice(0, count).map(target => ({ href: `/demo/post/${target}`, text: `>>${target}` })) });
      remaining -= count;
    }
    const rejected = String(start + 2000n);
    definitions.push({ id: rejected, quotes: [targets[0], owned.id].map(target => ({ href: `/demo/post/${target}`, text: `>>${target}` })) });
    await appendFixturePosts(page, owned.id, definitions);
    await expectRows(page, targets[0], admitted, owned.id);
    await expectRows(page, owned.id, [persisted], owned.id);
    expect(await page.locator('.board .backlink a.quotelink').count()).toBe(limits.edges);
    await expect(page.locator(`#m${rejected} a.quotelink`).last()).toHaveText(`>>${owned.id}`);
    await page.reload();
    await expectRows(page, owned.id, [persisted], owned.id);
  });

  test('the preview-row cap omits the whole oversized copy while preserving the live graph and a healthy popup', async ({ page, owned }) => {
    const target = await owned.reply('Body survives an oversized backlink copy');
    const source = await owned.reply(`>>${target}\n${trailingLines}`);
    await page.setViewportSize({ width: 1280, height: 440 });
    await initialize(page, owned.url);
    await expectRows(page, target, [source], owned.id);
    const limits = await activeLimits(page);
    expect(Number.isInteger(limits.previewRows) && limits.previewRows > 0).toBe(true);
    const extra = Array.from({ length: limits.previewRows }, (_, index) => String(900000000000000000n + BigInt(index)));
    await appendFixturePosts(page, owned.id, extra.map(id => ({ id, quotes: [{ href: `/demo/post/${target}`, text: `>>${target}` }] })));
    await expectRows(page, target, [source, ...extra], owned.id);
    const link = forward(page, source, target), requests = observeFetches(page);
    await link.evaluate(node => node.scrollIntoView({ block: 'start' }));
    await page.evaluate(() => scrollBy(0, -40));
    await link.hover();
    await expect(page.locator('#quote-preview .postMessage')).toHaveText('Body survives an oversized backlink copy');
    await expect(page.locator('#quote-preview .backlink')).toHaveCount(0);
    await expectRows(page, target, [source, ...extra], owned.id);
    expect(requests).toEqual([]);
    await page.reload();
    await expectRows(page, target, [source], owned.id);
    await link.evaluate(node => node.scrollIntoView({ block: 'start' }));
    await page.evaluate(() => scrollBy(0, -40));
    await link.hover();
    await expect(page.locator('#quote-preview .backlink a.quotelink')).toHaveCount(1);
  });

  test('catalog-shaped injected posts do not mount a graph and ordinary board navigation remains healthy', async ({ page, owned }) => {
    const source = await owned.reply(`>>${owned.id}\nHealthy catalog navigation control`);
    await initialize(page, '/demo/catalog');
    const requests = observeFetches(page);
    await page.evaluate(id => {
      const board = document.createElement('main'); board.className = 'board';
      const section = document.createElement('section'); section.className = 'thread'; section.id = `t${id}`;
      board.append(section); document.querySelector('.catalog').append(board);
    }, owned.id);
    await appendFixturePosts(page, owned.id, [
      { id: owned.id, text: 'Injected catalog target' },
      { id: '900000000000000001', quotes: [{ href: `/demo/post/${owned.id}`, text: `>>${owned.id}` }] },
    ]);
    await settledDOM(page);
    await expect(page.locator('.backlink')).toHaveCount(0);
    expect(requests).toEqual([]);
    await page.goto(owned.url);
    await expectRows(page, owned.id, [source], owned.id);
  });
});

test.describe('actual browser back-forward cache', () => {
  test('a real persisted pageshow restores one graph and honors settings changed while the document was cached', async ({ page, context, owned }) => {
    const source = await owned.reply(`>>${owned.id}\nActual back-forward cache source`);
    const cdp = await context.newCDPSession(page), rejected = [];
    await cdp.send('Page.enable');
    cdp.on('Page.backForwardCacheNotUsed', event => rejected.push(event.notRestoredExplanations));
    await page.addInitScript(() => {
      window.backlinkPageShows = [];
      addEventListener('pageshow', event => window.backlinkPageShows.push(event.persisted));
    });
    await initialize(page, owned.url);
    await expectRows(page, owned.id, [source], owned.id);
    await expectDesktopMenuOrder(page, [owned.id]);
    await expect.poll(() => page.evaluate(async () => (await navigator.locks.query()).held.length)).toBe(0);
    await forward(page, source, owned.id).evaluate(node => { window.cachedOriginalQuote = node; window.cachedOriginalDocument = document; });
    await page.goto('/demo/');
    // A restored document does not emit a new load event. Wait for navigation
    // commit, then require the actual persisted pageshow and original objects.
    await page.goBack({ waitUntil: 'commit' });
    await expect(page).toHaveURL(`${origin}${owned.url}`);
    await expect.poll(() => page.evaluate(() => window.backlinkPageShows.includes(true)), { message: JSON.stringify(rejected) }).toBe(true);
    expect(await page.evaluate(() => window.cachedOriginalDocument === document)).toBe(true);
    expect(await forward(page, source, owned.id).evaluate(node => node === window.cachedOriginalQuote)).toBe(true);
    await expectRows(page, owned.id, [source], owned.id);
    await expectDesktopMenuOrder(page, [owned.id]);
    await expect(forward(page, source, owned.id)).toHaveText(`>>${owned.id} (OP)`);
    await page.goto('/demo/');
    await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ backlinks: false })));
    await page.goBack({ waitUntil: 'commit' });
    await expect.poll(() => page.evaluate(() => window.backlinkPageShows.filter(Boolean).length), { message: JSON.stringify(rejected) }).toBe(2);
    await expectRows(page, owned.id, [], owned.id);
    await expect(forward(page, source, owned.id)).toHaveText(`>>${owned.id}`);
    const other = await context.newPage();
    try {
      await other.goto(owned.url);
      await saveSettings(other, { backlinks: true });
      await expectRows(page, owned.id, [source], owned.id);
      await expectDesktopMenuOrder(page, [owned.id]);
      await expect(forward(page, source, owned.id)).toHaveText(`>>${owned.id} (OP)`);
    } finally { await other.close(); await cdp.detach(); }
  });
});
