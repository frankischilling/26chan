import { test as base, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  fixture: async ({ request }, use) => {
    const password = 'owned-page-filter-password';
    const title = `Filter${Date.now().toString(36)}`;
    const post = async (resto, sub, com) => {
      const response = await request.post('/demo/post', { headers: { Origin: origin },
        form: { resto, sub, com, password }, maxRedirects: 0 });
      expect(response.status()).toBe(303);
      return response.headers().location.match(resto === '0' ? /thread\/(\d+)/ : /#p(\d+)/)[1];
    };
    const thread = await post('0', title, 'Owned filter opening post');
    try {
      const reply = await post(thread, '', 'Owned filter reply needle');
      await use({ thread, reply, title });
    } finally {
      expect((await request.post('/demo/delete', { headers: { Origin: origin },
        form: { no: thread, password }, maxRedirects: 0 })).status()).toBe(303);
      expect((await request.get(`/_watch/demo/thread/${thread}.json`)).status()).toBe(404);
    }
  },
});
const rule = (pattern, changes = {}) => ({ type: 2, pattern, boards: '', active: true, auto: false, hide: true, ...changes });
async function prepare(page, fixture, rules, settings = { filter: true }) {
  await page.goto(`/demo/thread/${fixture.thread}`);
  await page.evaluate(({ rules, settings }) => {
    localStorage.setItem('4chan-settings', JSON.stringify(settings));
    localStorage.setItem('4chan-filters', JSON.stringify(rules));
  }, { rules, settings });
  await page.reload();
}
async function editor(page) {
  await page.locator(page.viewportSize().width <= 480 ? '#settingsWindowLinkMobile' : '#settingsWindowLink').click();
  const category = page.getByRole('button', { name: 'Filters & Post Hiding', exact: true });
  if (await category.getAttribute('aria-expanded') === 'false') await category.click();
  await page.locator('#filters-edit').click();
  await expect(page.locator('#filtersMenu')).toBeVisible();
}

test('editor creates ordered native rules and palette values, then applies a global reply filter', async ({ page, fixture }) => {
  await prepare(page, fixture, []);
  await editor(page);
  await page.locator('[data-cmd=filters-add]').click();
  await page.getByLabel('Pattern for filter 1', { exact: true }).fill('needle');
  await page.getByLabel('Type for filter 1', { exact: true }).selectOption('2');
  await page.getByLabel('Hide filter 1', { exact: true }).check();
  await page.locator('.fColor').click();
  await page.getByRole('button', { name: '#E0B0FF', exact: true }).click();
  await page.locator('[data-cmd=filters-add]').click();
  await page.getByLabel('Pattern for filter 2', { exact: true }).fill('Other');
  await page.getByLabel('Move filter 2 up', { exact: true }).click();
  await expect(page.locator('#filter-list tr').first().locator('.fPattern')).toHaveValue('Other');
  await page.getByLabel('Delete filter 2', { exact: true }).click();
  await page.locator('[data-cmd=filters-save]').click();
  await expect(page.locator('#filtersMenu')).toHaveCount(0);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-filters')))).toEqual([
    rule('needle', { color: 'rgb(224, 176, 255)' }),
  ]);
  await page.getByRole('button', { name: 'Close settings', exact: true }).click();
  await expect(page.locator(`#p${fixture.reply}`)).toHaveClass(/post-hidden/);
  await expect(page.locator(`#m${fixture.reply}`)).toBeHidden();
  await page.getByRole('button', { name: `View filtered post ${fixture.reply}` }).click();
  await expect(page.locator(`#m${fixture.reply}`)).toBeVisible();
  await expect(page.locator(`#m${fixture.thread}`)).toBeVisible();
});

test('board subject filters hide OP threads, navigate through stubs, and never filter thread-page OPs', async ({ page, fixture }) => {
  await prepare(page, fixture, [rule(fixture.title, { type: 5 })]);
  await page.goto('/demo/');
  const section = page.locator(`#t${fixture.thread}`);
  await expect(section).toHaveClass(/post-hidden/);
  await page.getByRole('link', { name: `View filtered thread ${fixture.thread}` }).click();
  await expect(page).toHaveURL(new RegExp(`/demo/thread/${fixture.thread}$`));
  await expect(page.locator(`#m${fixture.thread}`)).toBeVisible();
  await expect(page.locator(`#m${fixture.reply}`)).toBeVisible();
  await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ filter: true, hideStubs: true })));
  await page.goto('/demo/');
  await expect(section).toBeHidden();
  await section.evaluate(element => { element.dataset.sticky = 'true'; });
  await page.evaluate(() => window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' })));
  await expect(section).toBeVisible();
  await expect(section).toHaveClass(/post-hidden/);
});

test('first matching rule highlights and tracked own replies are exempt', async ({ page, fixture }) => {
  await prepare(page, fixture, [rule('needle', { hide: false, color: '#ff0000' }), rule('needle')]);
  const reply = page.locator(`#p${fixture.reply}`);
  await expect(reply).toHaveClass(/filter-hl/);
  await expect(reply).toHaveCSS('box-shadow', 'rgb(255, 0, 0) -3px 0px 0px 0px');
  await expect(page.locator(`#m${fixture.reply}`)).toBeVisible();
  await page.evaluate(({ thread, reply }) => {
    localStorage.setItem(`4chan-track-demo-${thread}`, JSON.stringify({ [`>>${reply}`]: 1 }));
  }, fixture);
  await page.reload();
  await expect(reply).not.toHaveClass(/filter-hl|post-hidden/);
  await expect(page.locator('.nativeFilterNotice')).toBeEmpty();
});

test('invalid patterns and hostile colors fail visibly without injecting nodes or requesting resources', async ({ page, fixture }) => {
  const requests = [];
  page.on('request', request => { if (request.url().includes('filter-attack')) requests.push(request.url()); });
  await prepare(page, fixture, [rule('/[/')]);
  await expect(page.locator('.nativeFilterNotice')).toHaveText('Filters could not be applied. Posts are shown.');
  await expect(page.locator(`#m${fixture.reply}`)).toBeVisible();
  await editor(page);
  await page.locator('[data-cmd=filters-save]').click();
  await expect(page.locator('.filterEditorMessage')).toHaveText(/pattern is invalid/);
  await page.getByRole('button', { name: 'Close filters', exact: true }).click();
  await page.getByRole('button', { name: 'Close settings', exact: true }).click();
  await prepare(page, fixture, [rule('needle', { hide: false, color: 'red; background:url(/filter-attack)' })]);
  await expect(page.locator('.nativeFilterNotice')).toHaveText('Filters could not be applied. Posts are shown.');
  await expect(page.locator(`#m${fixture.reply}`)).toBeVisible();
  expect(requests).toEqual([]);
});

test('a competing tab cannot silently overwrite an open editor draft', async ({ page, context, fixture }) => {
  await prepare(page, fixture, []);
  await editor(page);
  await page.locator('[data-cmd=filters-add]').click();
  await page.getByLabel('Pattern for filter 1', { exact: true }).fill('Draft');
  const other = await context.newPage();
  await other.goto(`/demo/thread/${fixture.thread}`);
  const competing = [rule('needle')];
  await other.evaluate(rules => localStorage.setItem('4chan-filters', JSON.stringify(rules)), competing);
  await page.locator('[data-cmd=filters-save]').click();
  await expect(page.locator('.filterEditorMessage')).toHaveText(/Filters changed or could not be saved/);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-filters')))).toEqual(competing);
  await other.close();
});

test('settings activation survives navigation and disabling restores hidden replies', async ({ page, fixture }) => {
  await prepare(page, fixture, [rule('needle')], { filter: false });
  await editor(page);
  await page.getByRole('button', { name: 'Close filters', exact: true }).click();
  await page.locator('#setting-filter').check();
  await page.locator('#settings-save').click();
  await expect(page.locator(`#p${fixture.reply}`)).toHaveClass(/post-hidden/);
  await editor(page);
  await page.getByRole('button', { name: 'Close filters', exact: true }).click();
  await page.locator('#setting-filter').uncheck();
  await page.locator('#settings-save').click();
  await expect(page.locator(`#m${fixture.reply}`)).toBeVisible();
  await expect(page.locator(`#p${fixture.reply}`)).not.toHaveClass(/post-hidden/);
});

test('closing the editor cancels a save waiting for the shared storage lock', async ({ page, fixture }) => {
  await prepare(page, fixture, []);
  await editor(page);
  await page.locator('[data-cmd=filters-add]').click();
  await page.getByLabel('Pattern for filter 1', { exact: true }).fill('needle');
  await page.getByLabel('Type for filter 1', { exact: true }).selectOption('2');
  await page.evaluate(() => {
    window.filterLockHeld = new Promise(resolve => {
      window.filterLockDone = navigator.locks.request('paperboard-thread-watcher', async () => {
        await new Promise(release => { window.releaseFilterLock = release; resolve(); });
      });
    });
    return window.filterLockHeld;
  });
  await page.locator('[data-cmd=filters-save]').click();
  await expect.poll(() => page.evaluate(async () => (await navigator.locks.query()).pending
    .some(lock => lock.name === 'paperboard-thread-watcher'))).toBe(true);
  await page.getByRole('button', { name: 'Close filters', exact: true }).click();
  await page.evaluate(async () => { window.releaseFilterLock(); await window.filterLockDone; });
  await expect.poll(() => page.evaluate(async () => (await navigator.locks.query()).pending.length)).toBe(0);
  expect(await page.evaluate(() => localStorage.getItem('4chan-filters'))).toBe('[]');
});

test('unavailable writes retain editable same-tab filters without overwriting persisted rules', async ({ page, fixture }) => {
  await prepare(page, fixture, []);
  await page.evaluate(() => {
    const write = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (key === '4chan-filters') throw new DOMException('Test quota', 'QuotaExceededError');
      return write.call(this, key, value);
    };
  });
  await editor(page);
  await page.locator('[data-cmd=filters-add]').click();
  await page.getByLabel('Pattern for filter 1', { exact: true }).fill('needle');
  await page.getByLabel('Type for filter 1', { exact: true }).selectOption('2');
  await page.getByLabel('Hide filter 1', { exact: true }).check();
  await page.locator('[data-cmd=filters-save]').click();
  await expect(page.locator('.filterEditorMessage')).toHaveText('Filters are saved only in this tab. Browser storage or cross-tab locking is unavailable.');
  await expect(page.locator('#filtersMenu')).toBeVisible();
  // A second same-tab save uses the newly saved draft as its comparison value.
  await page.getByLabel('Pattern for filter 1', { exact: true }).fill('reply needle');
  await page.locator('[data-cmd=filters-save]').click();
  await expect(page.locator('[data-cmd=filters-save]')).toBeEnabled();
  await expect(page.locator('.filterEditorMessage')).toHaveText('Filters are saved only in this tab. Browser storage or cross-tab locking is unavailable.');
  await page.getByRole('button', { name: 'Close filters', exact: true }).click();
  await expect(page.locator('#filtersMenu')).toHaveCount(0);
  await page.getByRole('button', { name: 'Close settings', exact: true }).click();
  await expect(page.locator('.nativeFilterStorageNotice')).toBeVisible();
  await expect(page.locator('.nativeFilterStorageNotice')).toHaveText('Filters are saved only in this tab. Browser storage or cross-tab locking is unavailable.');
  await expect(page.locator(`#p${fixture.reply}`)).toHaveClass(/post-hidden/);
  expect(await page.evaluate(() => localStorage.getItem('4chan-filters'))).toBe('[]');
  await editor(page);
  await expect(page.getByLabel('Pattern for filter 1', { exact: true })).toHaveValue('reply needle');
  await page.getByLabel('Delete filter 1', { exact: true }).click();
  await page.locator('[data-cmd=filters-save]').click();
  await expect(page.locator('.filterEditorMessage')).toHaveText('Filters are saved only in this tab. Browser storage or cross-tab locking is unavailable.');
  await page.getByRole('button', { name: 'Close filters', exact: true }).click();
  await expect(page.locator('#filtersMenu')).toHaveCount(0);
  await page.getByRole('button', { name: 'Close settings', exact: true }).click();
  await expect(page.locator(`#m${fixture.reply}`)).toBeVisible();
  await page.reload();
  await expect(page.locator('.nativeFilterStorageNotice')).toBeHidden();
  await expect(page.locator(`#m${fixture.reply}`)).toBeVisible();
});

test('custom colors reject declarations and inherited values; stored patterns remain literal input text', async ({ page, fixture }) => {
  const pattern = '<img src=/filter-attack onerror=alert(1)>';
  const requests = [];
  page.on('request', request => { if (request.url().includes('filter-attack')) requests.push(request.url()); });
  await prepare(page, fixture, [rule(pattern, { active: false })]);
  await editor(page);
  await expect(page.locator('.fPattern')).toHaveValue(pattern);
  await expect(page.locator('#filtersMenu img, #filtersMenu script')).toHaveCount(0);
  await page.locator('.fColor').click();
  for (const value of ['red; background:url(/filter-attack)', 'var(--ink)', 'inherit', 'currentColor']) {
    await page.locator('#palette-custom-input').fill(value);
    await expect(page.locator('#palette-custom-ok')).toBeDisabled();
  }
  await page.locator('#palette-custom-input').fill('#0047ab');
  await page.locator('#palette-custom-ok').click();
  await expect(page.locator('.fColor')).toHaveCSS('background-color', 'rgb(0, 71, 171)');
  await page.locator('[data-cmd=filters-save]').click();
  await expect(page.locator('#filtersMenu')).toHaveCount(0);
  expect(requests).toEqual([]);
});

test('filter dialogs and nested help remain operable in six themes on desktop and mobile', async ({ page, context, fixture }) => {
  await prepare(page, fixture, [rule('needle')]);
  for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: origin, httpOnly: true, sameSite: 'Lax' }]);
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.reload();
      await editor(page);
      const bounds = await page.locator('#filtersMenu > .extPanel').boundingBox();
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
      await page.getByRole('button', { name: 'Filter help', exact: true }).click();
      await expect(page.locator('#filtersHelp')).toBeVisible();
      await page.keyboard.press('Escape');
      await expect(page.locator('#filtersHelp')).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Filter help', exact: true })).toBeFocused();
      await page.getByRole('button', { name: 'Close filters', exact: true }).click();
      await expect(page.locator('#filters-edit')).toBeFocused();
      await page.getByRole('button', { name: 'Close settings', exact: true }).click();
    }
  }
});

async function selectText(page, selector) {
  await page.locator(selector).evaluate(element => {
    const text = element.firstChild;
    const range = document.createRange(); range.setStart(text, 0); range.setEnd(text, text.textContent.length);
    const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
  });
}

test('post-menu selection opens an unsaved native Name filter and saves through the existing editor', async ({ page, fixture }) => {
  await prepare(page, fixture, []);
  await selectText(page, `#p${fixture.reply} .name`);
  await page.getByRole('button', { name: `Post menu for post ${fixture.reply}`, exact: true }).click();
  await page.getByRole('menuitem', { name: 'Filter selected text', exact: true }).click();
  await expect(page.locator('#filter-list tr')).toHaveCount(1);
  await expect(page.locator('.fPattern')).toHaveValue('Anonymous');
  await expect(page.locator('#filter-list select')).toHaveValue('1');
  await expect(page.locator('.fBoards')).toHaveValue('');
  await expect(page.getByLabel('Hide filter 1', { exact: true })).not.toBeChecked();
  expect(await page.evaluate(() => localStorage.getItem('4chan-filters'))).toBe('[]');
  await page.getByLabel('Hide filter 1', { exact: true }).check();
  await page.locator('[data-cmd=filters-save]').click();
  await expect(page.locator('#filtersMenu')).toHaveCount(0);
  await expect(page.locator(`#p${fixture.reply}`)).toHaveClass(/post-hidden/);
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('4chan-filters')))).toEqual([rule('Anonymous', { type: 1 })]);
});

test('selection infers native field types, trims text and cancelling never changes saved rules', async ({ page, fixture }) => {
  await prepare(page, fixture, []);
  // Tripcode, ID and filename nodes are explicit UI fixtures, not backend posting claims.
  await page.locator(`#p${fixture.thread} .postInfo`).evaluate(info => {
    for (const [className, text] of [['postertrip', '!OwnedTrip'], ['hand', 'OwnedID'], ['fileText', 'owned-file.png']]) {
      const span = document.createElement('span'); span.className = className; span.textContent = text; info.append(span);
    }
  });
  const cases = [[`.subject`, fixture.title, '5'], ['.postertrip', '!OwnedTrip', '0'], ['.hand', 'OwnedID', '4'],
    ['.fileText', 'owned-file.png', '6'], ['.postMessage', 'Owned filter opening post', '2']];
  for (const [selector, text, type] of cases) {
    await selectText(page, `#p${fixture.thread} ${selector}`);
    await page.getByRole('button', { name: `Post menu for post ${fixture.thread}`, exact: true }).click();
    await page.getByRole('menuitem', { name: 'Filter selected text', exact: true }).click();
    await expect(page.locator('.fPattern')).toHaveValue(text);
    await expect(page.locator('#filter-list select')).toHaveValue(type);
    await page.getByRole('button', { name: 'Close filters', exact: true }).click();
    await expect(page.getByRole('button', { name: `Post menu for post ${fixture.thread}`, exact: true })).toBeFocused();
    expect(await page.evaluate(() => localStorage.getItem('4chan-filters'))).toBe('[]');
  }
});

test('mobile selection stays literal and oversized selections cannot create truncated filters', async ({ page, fixture }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await prepare(page, fixture, []);
  for (const value of ['  <img src=/selection-attack onerror=alert(1)>  ', 'x'.repeat(1025)]) {
    await page.locator(`#m${fixture.reply}`).evaluate((element, value) => { element.textContent = value; }, value);
    await selectText(page, `#m${fixture.reply}`);
    await page.getByRole('button', { name: `Post menu for post ${fixture.reply}`, exact: true }).click();
    await page.getByRole('menuitem', { name: 'Filter selected text', exact: true }).click();
    if (value.length > 1024) {
      await expect(page.locator('#filter-list tr')).toHaveCount(0);
      await expect(page.locator('.filterEditorMessage')).toHaveText(/Selected text exceeds/);
    } else {
      await expect(page.locator('.fPattern')).toHaveValue(value.trim());
      await expect(page.locator('#filtersMenu img, #filtersMenu script')).toHaveCount(0);
    }
    await page.getByRole('button', { name: 'Close filters', exact: true }).click();
    expect(await page.evaluate(() => localStorage.getItem('4chan-filters'))).toBe('[]');
  }
});

test('an open menu removes its filter action when another tab disables filtering', async ({ page, context, fixture }) => {
  await prepare(page, fixture, []);
  await page.getByRole('button', { name: `Post menu for post ${fixture.reply}`, exact: true }).click();
  await expect(page.getByRole('menuitem', { name: 'Filter selected text', exact: true })).toBeVisible();
  const other = await context.newPage();
  await other.goto(`/demo/thread/${fixture.thread}`);
  await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ filter: false })));
  await expect(page.getByRole('menuitem', { name: 'Filter selected text', exact: true })).toHaveCount(0);
  await other.close();
});
