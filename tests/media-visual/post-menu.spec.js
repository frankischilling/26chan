import { test, expect } from '../helpers/visual-diagnostics.js';

test.use({ javaScriptEnabled: true });
const id = '1000201';
const trigger = page => page.getByRole('button', { name: `Post menu for post ${id}`, exact: true });
const providerRequest = url => /https:\/\/(lens\.google\.com|www\.yandex\.com|saucenao\.com)\//.test(url);
async function checkProviderLink(link, name, file) {
  const endpoint = { Google: 'https://lens.google.com/uploadbyurl',
    Yandex: 'https://www.yandex.com/images/search', SauceNAO: 'https://saucenao.com/search.php' }[name];
  const url = new URL(await link.getAttribute('href'));
  expect(url.origin + url.pathname).toBe(endpoint);
  expect([...url.searchParams]).toEqual(name === 'Yandex'
    ? [['img_url', file], ['rpt', 'imageview']] : [['url', file]]);
  await expect(link).toHaveAttribute('target', '_blank');
  await expect(link).toHaveAttribute('rel', 'noopener noreferrer');
}

test('desktop image search uses the full normalized file and supports nested keyboard navigation', async ({ page }) => {
  const external = [];
  page.on('request', request => { if (/https:\/\/(lens\.google\.com|www\.yandex\.com|saucenao\.com)\//.test(request.url())) external.push(request.url()); });
  await page.goto('/img/thread/1000201');
  const file = await page.locator(`#p${id} .fileThumb`).getAttribute('href');
  const toggle = page.getByRole('menuitem', { name: 'Image search', exact: true });
  await trigger(page).press('ArrowUp');
  await expect(toggle).toBeFocused();
  await expect(page.getByRole('menu', { name: 'Image search providers' })).toBeHidden();
  await page.keyboard.press('Escape');
  await trigger(page).press('ArrowDown');
  await toggle.focus(); await toggle.press('ArrowRight');
  await expect(page.getByRole('menuitem', { name: 'Google', exact: true })).toBeFocused();
  for (const name of ['Google', 'Yandex', 'SauceNAO']) {
    const link = page.getByRole('menuitem', { name, exact: true });
    await checkProviderLink(link, name, file);
  }
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'Yandex', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowLeft'); await expect(toggle).toBeFocused();
  await expect(page.getByRole('menu', { name: 'Image search providers' })).toBeHidden();
  await page.keyboard.press('Escape'); await expect(trigger(page)).toBeFocused();
  expect(external).toEqual([]);
});

test('mobile file and post deletion confirmations cancel without changing the actual form or submitting', async ({ page }) => {
  const external = [], mutations = [];
  page.on('request', request => {
    if (providerRequest(request.url())) external.push(request.url());
    if (request.method() === 'POST') mutations.push(request.url());
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/img/thread/1000201');
  await trigger(page).click();
  await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveAttribute('href', 'http://localhost:3004/img/1000201.png');
  for (const name of ['Google', 'Yandex', 'SauceNAO']) {
    const link = page.getByRole('menuitem', { name: `Search image on ${name}`, exact: true });
    await expect(link).toBeVisible();
    await checkProviderLink(link, name, 'http://localhost:3004/img/1000201.png');
  }
  const form = page.locator(`#p${id} form[action="/img/delete"]`);
  const fields = await form.evaluate(form => [...new FormData(form)]);
  for (const action of ['Delete file', 'Delete post']) {
    if (action === 'Delete post') await trigger(page).click();
    const confirmation = page.waitForEvent('dialog');
    const clicked = page.getByRole('menuitem', { name: action, exact: true }).click();
    const dialog = await confirmation;
    const message = dialog.message(), type = dialog.type();
    await dialog.dismiss();
    await clicked;
    expect(type).toBe('confirm');
    expect(message).toBe(`${action}?`);
    expect(await form.evaluate(form => [...new FormData(form)])).toEqual(fields);
    await expect(page.locator(`#pc${id}.deleted, #f${id}.deleted, #f${id} img.deleted`)).toHaveCount(0);
    await expect(page).toHaveURL(/\/img\/thread\/1000201$/);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole('button', { name: 'Post menu for post 1000206', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: 'Delete file', exact: true })).toHaveCount(0);
  await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveCount(0);
  expect(external).toEqual([]);
  expect(mutations).toEqual([]);
});

test('spoiler file actions use metadata without revealing or fetching the hidden image', async ({ page }) => {
  const requests = [];
  page.on('request', request => requests.push(request.url()));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/img/thread/1000201');
  const post = page.locator('#p1000205');
  await expect(post.locator('.imgspoiler img')).toHaveAttribute('src', '/static/catalog/spoiler.png');
  await page.getByRole('button', { name: 'Post menu for post 1000205', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveAttribute('href', 'http://localhost:3004/img/1000205.png');
  await expect(page.getByRole('menuitem', { name: 'Delete file', exact: true })).toBeVisible();
  for (const name of ['Google', 'Yandex', 'SauceNAO']) {
    const link = page.getByRole('menuitem', { name: `Search image on ${name}`, exact: true });
    await checkProviderLink(link, name, 'http://localhost:3004/img/1000205.png');
  }
  await expect(post.locator('.imgspoiler')).toBeVisible();
  await expect(post.locator('.fileThumb:not(.imgspoiler)')).toHaveCount(0);
  expect(requests.some(url => /\/img\/1000205(?:s\.jpg|\.png)$/.test(url))).toBe(false);
  expect(requests.some(providerRequest)).toBe(false);
});

test('noncanonical and credential-bearing file links cannot become menu navigation', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/img/thread/1000201');
  for (const href of ['javascript:void(0)', 'https://synthetic:synthetic@example.invalid/img/1000201.png',
    'https://example.invalid/wrong/1000201.png', 'https://example.invalid/img/1000201.png?token=synthetic']) {
    await page.locator(`#p${id} .file > .fileText > a`).evaluate((link, href) => { link.href = href; }, href);
    await trigger(page).click();
    await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveCount(0);
    await expect(page.getByRole('menuitem', { name: 'Search image on Google', exact: true })).toHaveCount(0);
    await page.keyboard.press('Escape');
  }
});
