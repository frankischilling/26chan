import { test, expect } from '@playwright/test';

function synthetic(corrupt = false, ranked = false) {
  const cards = [
    ['1', '0', '1', '0', true],
    ['9007199254740993', '100', '9007199254741000', '2', false],
    ['9007199254740992', corrupt ? 'invalid' : '100', '9007199254741001', '2', false],
    ['9007199254740994', '99', '9007199254741002', '3', false],
    ['3', '98', '', '0', false],
    ['2', '98', '', '0', false],
  ];
  return `<!doctype html><form id="ctrl" action="/fixture/catalog" method="get">
    <select id="order-ctrl" name="order"><option value="alt">Bump</option><option value="date">Creation</option><option value="absdate">Last reply</option><option value="r">Replies</option></select>
    <select id="size-ctrl" name="size"><option value="small">Small</option><option value="large">Large</option></select>
    <select id="teaser-ctrl" name="teaser"><option value="on">On</option><option value="off">Off</option></select>
    <a id="catalog-reset" href="/fixture/catalog">Reset</a></form>
    <div id="threads" class="catalog extended-small">${cards.map(([id,bump,latest,replies,sticky], index) =>
      `<section class="thread" id="thread-${id}" data-thread-id="${id}" ${ranked ? `data-bump-position="${[0,2,1,3,4,5][index]}" ` : ''}data-bumped="${bump}" data-latest-reply="${latest}" data-replies="${replies}" data-sticky="${sticky || ranked && index < 3}"><div class="teaser">Synthetic ${id}</div></section>`).join('\n')}</div>
    <script type="module" src="/static/catalog-preferences.v1.js"></script>`;
}

test('in-place catalog ranks retain integer precision, sticky priority and tie ordering', async ({ page }) => {
  const actual = await page.goto('/fixture/catalog');
  const csp = actual.headers()['content-security-policy'];
  await page.route('**/fixture/catalog', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: synthetic() }));
  await page.goto('/fixture/catalog');
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations += 1; });
  const a = '9007199254740992', b = '9007199254740993', c = '9007199254740994';
  for (const [order, expected] of [['date',['1',c,b,a,'3','2']], ['absdate',['1',c,a,b,'2','3']], ['r',['1',c,a,b,'2','3']], ['alt',['1',b,a,c,'3','2']]]) {
    await page.locator('#order-ctrl').selectOption(order);
    expect(await page.locator('.thread').evaluateAll(nodes => nodes.map(node => node.dataset.threadId))).toEqual(expected);
  }
  expect(navigations).toBe(0);
});

test('invalid catalog metadata falls back to the real validated GET form', async ({ page }) => {
  const actual = await page.goto('/fixture/catalog');
  const csp = actual.headers()['content-security-policy'];
  await page.route('**/fixture/catalog', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: synthetic(true) }));
  await page.goto('/fixture/catalog');
  const navigation = page.waitForResponse(response => response.request().isNavigationRequest() && new URL(response.url()).searchParams.get('size') === 'large');
  await page.locator('#size-ctrl').selectOption('large');
  expect((await navigation).status()).toBe(200);
  await expect(page.locator('#threads')).toHaveClass('catalog extended-large');
  expect(await page.locator('[data-thread-id="9007199254740993"]').count()).toBe(0);
});

test('catalog returns to original ranked positions after other sorts without reloading', async ({ page }) => {
  const actual = await page.goto('/fixture/catalog');
  const csp = actual.headers()['content-security-policy'];
  await page.route('**/fixture/catalog', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: synthetic(false, true) }));
  await page.goto('/fixture/catalog');
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations++; });
  for (const alternate of ['date', 'r', 'absdate']) {
    await page.locator('#order-ctrl').selectOption(alternate);
    await page.locator('#order-ctrl').selectOption('alt');
    expect(await page.locator('.thread').evaluateAll(nodes => nodes.map(node => node.dataset.threadId)))
      .toEqual(['1', '9007199254740992', '9007199254740993', '9007199254740994', '3', '2']);
  }
  expect(navigations).toBe(0);
});

function interactiveRanked(textOnly = false) {
  const form = synthetic().split('<div id="threads"')[0]
    .replace('</form>', '<input id="qf-box" name="q" type="search"></form>');
  const rows = [10, 20, 30].map((id, position) => {
    const subject = `Owned ranked ${id}`;
    const attributes = `class="thread" id="thread-${id}" data-thread-id="${id}" data-bump-position="${position}" data-bumped="${100 + position}" data-latest-reply="${id}" data-replies="${position}" data-sticky="${id !== 30}"`;
    const link = `<a class="catalogThumb" href="/fixture/thread/${id}" data-search-text="${subject}" data-search-file="" data-has-file="false">${subject}</a>`;
    const teaser = `<div class="teaser">${subject}</div>`;
    return textOnly
      ? `<tr ${attributes}><td>${link}<template class="catalogTeaser">${teaser}</template></td><td class="txt-rep"><span data-replies-count>${position}</span></td><td class="txt-ctrl"></td></tr>`
      : `<section ${attributes}>${link}<div class="meta"><b>${position}</b></div>${teaser}</section>`;
  }).join('');
  return `${form}<div id="threads" class="catalog" data-text-only="${textOnly}" data-threads-per-page="1">${textOnly ? `<table><tbody>${rows}</tbody></table>` : rows}</div><template id="catalogFiltered"></template><script type="module" src="/static/catalog-preferences.v1.js"></script>`;
}

test('ranked text catalogs restore original positions after creation order', async ({ page }) => {
  const actual = await page.goto('/fixture/catalog');
  const csp = actual.headers()['content-security-policy'];
  await page.route('**/fixture/catalog', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: interactiveRanked(true) }));
  await page.goto('/fixture/catalog');
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations++; });
  const rows = page.locator('#threads tbody > .thread');
  await page.locator('#order-ctrl').selectOption('date');
  expect(await rows.evaluateAll(nodes => nodes.map(node => node.dataset.threadId))).toEqual(['30', '20', '10']);
  await page.locator('#order-ctrl').selectOption('alt');
  expect(await rows.evaluateAll(nodes => nodes.map(node => node.dataset.threadId))).toEqual(['10', '20', '30']);
  expect(navigations).toBe(0);
});

test('pinning a lower-ranked sticky preserves its rank and original page badge', async ({ page }) => {
  const actual = await page.goto('/fixture/catalog');
  const csp = actual.headers()['content-security-policy'];
  await page.route('**/fixture/catalog', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: interactiveRanked() }));
  await page.goto('/fixture/catalog');
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations++; });
  await page.getByRole('button', { name: 'Thread 20 menu', exact: true }).click();
  await page.getByRole('menuitem', { name: 'Pin thread', exact: true }).click();
  await expect(page.locator('#thread-20 .catalogPinPage')).toHaveText(' / P: 2');
  const ids = () => page.locator('#threads > .thread').evaluateAll(nodes => nodes.map(node => node.dataset.threadId));
  expect(await ids()).toEqual(['10', '20', '30']);
  await page.locator('#order-ctrl').selectOption('date');
  expect(await ids()).toEqual(['20', '10', '30']);
  await page.locator('#order-ctrl').selectOption('alt');
  expect(await ids()).toEqual(['10', '20', '30']);
  await expect(page.locator('#thread-20 .catalogPinPage')).toHaveText(' / P: 2');
  expect(navigations).toBe(0);
});

for (const [name, replacement] of [
  ['partial', ''], ['malformed', 'data-bump-position="bad"'],
  ['duplicate', 'data-bump-position="1"'], ['unbounded', 'data-bump-position="1000"'],
]) {
  test(`${name} catalog positions fall back to the real validated GET form`, async ({ page }) => {
    const actual = await page.goto('/fixture/catalog');
    const csp = actual.headers()['content-security-policy'];
    const body = synthetic(false, true).replace('data-bump-position="0"', replacement);
    await page.route('**/fixture/catalog', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body }));
    await page.goto('/fixture/catalog');
    const navigation = page.waitForResponse(response => response.request().isNavigationRequest() && new URL(response.url()).searchParams.get('size') === 'large');
    await page.locator('#size-ctrl').selectOption('large');
    expect((await navigation).status()).toBe(200);
    await expect(page.locator('#threads')).toHaveClass('catalog extended-large');
    expect(await page.locator('[data-thread-id="9007199254740993"]').count()).toBe(0);
  });
}
