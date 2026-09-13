import { test, expect } from '@playwright/test';

function synthetic(corrupt = false) {
  const cards = [
    ['1', '0', '1', '0', true],
    ['9007199254740993', '100', '9007199254741000', '2', false],
    ['9007199254740992', corrupt ? 'invalid' : '100', '9007199254741001', '2', false],
    ['9007199254740994', '99', '9007199254741002', '3', false],
    ['3', '98', '', '0', false],
    ['2', '98', '', '0', false],
  ];
  return `<!doctype html><form id="ctrl" action="/test/catalog" method="get">
    <select id="order-ctrl" name="order"><option value="alt">Bump</option><option value="date">Creation</option><option value="absdate">Last reply</option><option value="r">Replies</option></select>
    <select id="size-ctrl" name="size"><option value="small">Small</option><option value="large">Large</option></select>
    <select id="teaser-ctrl" name="teaser"><option value="on">On</option><option value="off">Off</option></select>
    <a id="catalog-reset" href="/test/catalog">Reset</a></form>
    <div id="threads" class="catalog extended-small">${cards.map(([id,bump,latest,replies,sticky]) =>
      `<section class="thread" id="thread-${id}" data-thread-id="${id}" data-bumped="${bump}" data-latest-reply="${latest}" data-replies="${replies}" data-sticky="${sticky}"><div class="teaser">Synthetic ${id}</div></section>`).join('\n')}</div>
    <script src="/static/catalog-preferences.v1.js" defer></script>`;
}

test('in-place catalog ranks retain integer precision, sticky priority and tie ordering', async ({ page }) => {
  const actual = await page.goto('/test/catalog');
  const csp = actual.headers()['content-security-policy'];
  await page.route('**/test/catalog', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: synthetic() }));
  await page.goto('/test/catalog');
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
  const actual = await page.goto('/test/catalog');
  const csp = actual.headers()['content-security-policy'];
  await page.route('**/test/catalog', route => route.fulfill({ contentType: 'text/html', headers: { 'content-security-policy': csp }, body: synthetic(true) }));
  await page.goto('/test/catalog');
  const navigation = page.waitForResponse(response => response.request().isNavigationRequest() && new URL(response.url()).searchParams.get('size') === 'large');
  await page.locator('#size-ctrl').selectOption('large');
  expect((await navigation).status()).toBe(200);
  await expect(page.locator('#threads')).toHaveClass('catalog extended-large');
  expect(await page.locator('[data-thread-id="9007199254740993"]').count()).toBe(0);
});
