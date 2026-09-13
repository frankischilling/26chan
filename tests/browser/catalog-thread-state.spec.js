import { test, expect } from '@playwright/test';

const a = '9007199254740993';
const b = '9007199254740994';
const c = '9007199254740995';
const d = '9007199254740996';
const pinKey = '4chan-pin-test';
const hideKey = '4chan-hide-t-test';

async function fixture(page, stored = {}, replies = { [a]: 30, [b]: 10, [c]: 20, [d]: 0 }) {
  const response = await page.goto('/test/catalog?q=');
  const csp = response.headers()['content-security-policy'];
  await page.evaluate(({ stored, pinKey, hideKey }) => {
    localStorage.removeItem(pinKey);
    localStorage.removeItem(hideKey);
    for (const [key, value] of Object.entries(stored)) localStorage.setItem(key, value);
  }, { stored, pinKey, hideKey });
  const card = (id, index) => {
    const subject = ['Alpha', 'Bravo', 'Crane', 'Sticky'][index];
    return `<section class="thread" id="thread-${id}" data-thread-id="${id}" data-bumped="${100 + index}" data-latest-reply="${index === 0 ? '9007199254741099' : id}" data-replies="${replies[id]}" data-sticky="${id === d}"><a class="catalogThumb" href="/test/thread/${id}" data-search-subject="${subject}" data-search-comment="${subject}" data-search-file="" data-has-file="false"><img class="thumb nofile" id="thumb-${id}" src="/static/catalog/nofile.png" width="77" height="13" data-small-width="77" data-small-height="13" data-large-width="77" data-large-height="13"></a><div class="meta" id="meta-${id}" title="(R)eplies / (I)mage Replies">R: <b>${replies[id]}</b></div><div class="teaser">${subject}</div></section>`;
  };
  await page.route('**/test/catalog?thread-state=*', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: `<!doctype html><link rel="stylesheet" href="/static/board.css"><link rel="stylesheet" href="/static/theme.css?worksafe=true"><form id="ctrl" action="/test/catalog" method="get"><select id="order-ctrl" name="order"><option value="alt">Bump</option><option value="date">Creation</option><option value="absdate">Reply</option><option value="r">Count</option></select><select id="size-ctrl" name="size"><option value="small">Small</option><option value="large">Large</option></select><select id="teaser-ctrl" name="teaser"><option value="on">On</option><option value="off">Off</option></select><input id="qf-box" name="q" type="search"><button>Apply</button><a id="catalog-reset" href="/test/catalog">Reset</a></form><div id="threads" class="catalog extended-small" data-threads-per-page="2">${[d, c, b, a].map(id => card(id, [a, b, c, d].indexOf(id))).join('\n')}</div><template id="catalogFiltered"></template><script src="/static/catalog-preferences.v1.js" defer></script>` }));
  await page.goto('/test/catalog?thread-state=owned&q=');
}

const ids = page => page.locator('#threads > .thread').evaluateAll(nodes => nodes.map(node => node.dataset.threadId));
async function action(page, id, name) {
  await page.locator(`#thread-${id}`).hover();
  await page.getByRole('button', { name: `Thread ${id} menu`, exact: true }).click();
  await page.getByRole('menuitem', { name, exact: true }).click();
}

test('pins preserve exact IDs, sticky priority, selected sort order and original page positions', async ({ page }) => {
  await fixture(page);
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest()) navigations += 1; });
  await action(page, a, 'Pin thread');
  expect(await ids(page)).toEqual([d, a, c, b]);
  await expect(page.locator(`#thumb-${a}`)).toHaveClass(/pinned/);
  await expect(page.locator(`#meta-${a} .catalogPinDelta`)).toHaveText('(+0)');
  await expect(page.locator(`#meta-${a} .catalogPinPage`)).toHaveText(' / P: 2');
  await action(page, b, 'Pin thread');
  for (const order of ['date', 'r', 'absdate', 'alt']) {
    await page.locator('#order-ctrl').selectOption(order);
    expect(await ids(page)).toEqual(['r', 'absdate'].includes(order) ? [d, a, b, c] : [d, b, a, c]);
  }
  const saved = await page.evaluate(key => JSON.parse(localStorage.getItem(key)), pinKey);
  expect(saved).toEqual({ [a]: 30, [b]: 10 });
  await page.getByRole('button', { name: 'Unpin all threads', exact: true }).click();
  expect(await ids(page)).toEqual([d, c, b, a]);
  expect(await page.evaluate(key => localStorage.getItem(key), pinKey)).toBeNull();
  expect(navigations).toBe(0);
});

test('search can surface hidden threads while the hidden view takes precedence over search', async ({ page }) => {
  await fixture(page);
  await page.locator(`#thumb-${a}`).click({ modifiers: ['Shift'] });
  expect(await ids(page)).toEqual([d, c, b]);
  await expect(page.locator('#hidden-count')).toHaveText('1');
  await page.locator('#qf-box').fill('Alpha');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  expect(await ids(page)).toEqual([a]);
  await expect(page.locator('#hidden-label')).toBeHidden();
  await page.locator('#qf-box').press('Escape');
  expect(await ids(page)).toEqual([d, c, b]);
  await page.locator('#filters-clear-hidden').click();
  expect(await ids(page)).toEqual([a]);
  await expect(page.locator('#filters-clear-hidden')).toHaveText('Back');
  await page.locator('#qf-box').fill('Bravo');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  expect(await ids(page)).toEqual([a]);
  await action(page, a, 'Unhide thread');
  expect(await ids(page)).toEqual([b]);
  await expect(page.locator('#hidden-label')).toBeHidden();
  expect(await page.evaluate(key => localStorage.getItem(key), hideKey)).toBeNull();
});

test('pin reply deltas survive reload and storage pruning retains newer absent IDs', async ({ page }) => {
  const future = '9223372036854775000';
  const replies = { [a]: 30, [b]: 10, [c]: 20, [d]: 0 };
  await fixture(page, {
    [pinKey]: JSON.stringify({ 1: 0, [a]: 25, [future]: 0, constructor: 0 }),
    [hideKey]: JSON.stringify({ 2: true, [c]: true, [future]: true, bad: true }),
  }, replies);
  await expect(page.locator(`#meta-${a} .catalogPinDelta`)).toHaveText(' (+5)');
  expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key)), pinKey)).toEqual({ [a]: 30, [future]: 0 });
  expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key)), hideKey)).toEqual({ [c]: true, [future]: true });
  replies[a] = 37;
  await page.reload();
  await expect(page.locator(`#meta-${a} .catalogPinDelta`)).toHaveText(' (+7)');
  await page.locator('#size-ctrl').selectOption('large');
  await expect(page.locator(`#meta-${a} .catalogPinDelta`)).toHaveText('(+0)');
  await page.goto('/demo/catalog?q=');
  expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key)), pinKey)).toEqual({ [a]: 37, [future]: 0 });
});

test('menus support keyboard dismissal, report links, context clicks and optional storage', async ({ page }) => {
  await fixture(page);
  await page.evaluate(() => {
    for (const name of ['getItem', 'setItem', 'removeItem']) Object.defineProperty(Storage.prototype, name, { value() { throw new DOMException('Unavailable', 'SecurityError'); } });
  });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const button = page.getByRole('button', { name: `Thread ${a} menu`, exact: true });
  await button.focus();
  await button.press('Enter');
  const report = page.getByRole('menuitem', { name: 'Report thread', exact: true });
  await expect(report).toBeFocused();
  await expect(report).toHaveAttribute('href', new RegExp(`/test/thread/${a}#report${a}$`));
  await report.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'Pin thread', exact: true })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(button).toBeFocused();
  await expect(page.getByRole('menu')).toHaveCount(0);
  await page.locator(`#thumb-${a}`).click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Pin thread', exact: true }).click();
  expect(await ids(page)).toEqual([d, a, c, b]);
  await page.locator(`#thumb-${a}`).click({ modifiers: ['Alt'] });
  expect(await ids(page)).toEqual([d, c, b, a]);
  await action(page, a, 'Hide thread');
  await page.locator('#filters-clear-hidden').click();
  await action(page, a, 'Unhide thread');
  expect(await ids(page)).toEqual([d, c, b, a]);
  expect(errors).toEqual([]);
});

test('hiding every card remains reversible and search-menu unhide performs its labeled action', async ({ page }) => {
  await fixture(page);
  for (const id of [a, b, c, d]) await page.locator(`#thumb-${id}`).click({ modifiers: ['Shift'] });
  await expect(page.locator('#threads > .empty')).toHaveText('All threads are hidden. Show hidden threads.');
  await page.getByRole('link', { name: 'Show hidden threads', exact: true }).click();
  expect(await ids(page)).toEqual([d, c, b, a]);
  await page.locator('#filters-clear-hidden-bottom').click();
  await page.locator('#qf-box').fill('Alpha');
  await page.getByRole('button', { name: 'Apply', exact: true }).click();
  expect(await ids(page)).toEqual([a]);
  await action(page, a, 'Unhide thread');
  expect(await ids(page)).toEqual([a]);
  expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key)), hideKey)).toEqual({ [b]: true, [c]: true, [d]: true });
});

test('malformed and oversized thread state cannot disable controls or become executable data', async ({ page }) => {
  for (const raw of ['null', '[]', '{', 'x'.repeat(65537), JSON.stringify(Object.fromEntries(Array.from({ length: 1025 }, (_, index) => [String(index + 1), 0])))]) {
    await fixture(page, { [pinKey]: raw, [hideKey]: raw });
    expect(await ids(page)).toEqual([d, c, b, a]);
    await action(page, a, 'Pin thread');
    expect(await page.evaluate(key => JSON.parse(localStorage.getItem(key)), pinKey)).toEqual({ [a]: 30 });
  }
});
