import { test, expect } from '@playwright/test';

test.use({ javaScriptEnabled: true });
const id = '1000201';
const trigger = page => page.getByRole('button', { name: `Post menu for post ${id}`, exact: true });

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
  for (const [name, endpoint] of [['Google', 'https://lens.google.com/uploadbyurl'], ['Yandex', 'https://www.yandex.com/images/search'], ['SauceNAO', 'https://saucenao.com/search.php']]) {
    const link = page.getByRole('menuitem', { name, exact: true });
    const url = new URL(await link.getAttribute('href'));
    expect(url.origin + url.pathname).toBe(endpoint);
    expect(url.searchParams.get('url')).toBe(file);
    if (name === 'Yandex') expect(url.searchParams.get('rpt')).toBe('imageview');
    await expect(link).toHaveAttribute('target', '_blank');
    await expect(link).toHaveAttribute('rel', 'noopener noreferrer');
  }
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: 'Yandex', exact: true })).toBeFocused();
  await page.keyboard.press('ArrowLeft'); await expect(toggle).toBeFocused();
  await expect(page.getByRole('menu', { name: 'Image search providers' })).toBeHidden();
  await page.keyboard.press('Escape'); await expect(trigger(page)).toBeFocused();
  expect(external).toEqual([]);
});

test('mobile file actions select but never submit the actual deletion form', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/img/thread/1000201');
  await trigger(page).click();
  await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveAttribute('href', 'http://localhost:3004/img/1000201.png');
  for (const name of ['Google', 'Yandex', 'SauceNAO']) await expect(page.getByRole('menuitem', { name: `Search image on ${name}`, exact: true })).toBeVisible();
  await page.getByRole('menuitem', { name: 'Delete file', exact: true }).click();
  const form = page.locator(`#p${id} form[action="/img/delete"]`);
  await expect(form.locator('[name=file_only]')).toBeChecked();
  await expect(form.locator('[name=password]')).toBeFocused();
  await expect(page).toHaveURL(/\/img\/thread\/1000201$/);
  await trigger(page).click(); await page.getByRole('menuitem', { name: 'Delete post', exact: true }).click();
  await expect(form.locator('[name=file_only]')).not.toBeChecked();
  await expect(form.locator('[name=password]')).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole('button', { name: 'Post menu for post 1000206', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: 'Delete file', exact: true })).toHaveCount(0);
  await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveCount(0);
});

test('spoiler file actions use metadata without revealing or fetching the hidden image', async ({ page }) => {
  const requests = [];
  page.on('request', request => requests.push(request.url()));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/img/thread/1000201');
  const post = page.locator('#p1000205');
  await expect(post.locator('.fileThumb')).toHaveCount(0);
  await page.getByRole('button', { name: 'Post menu for post 1000205', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveAttribute('href', 'http://localhost:3004/img/1000205.png');
  await expect(page.getByRole('menuitem', { name: 'Delete file', exact: true })).toBeVisible();
  for (const name of ['Google', 'Yandex', 'SauceNAO']) {
    const url = new URL(await page.getByRole('menuitem', { name: `Search image on ${name}`, exact: true }).getAttribute('href'));
    expect(url.searchParams.get('url')).toBe('http://localhost:3004/img/1000205.png');
  }
  await expect(post.locator('.file details')).not.toHaveAttribute('open');
  expect(requests.some(url => /\/img\/1000205(?:s\.jpg|\.png)$/.test(url))).toBe(false);
});

test('noncanonical and credential-bearing file links cannot become menu navigation', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/img/thread/1000201');
  for (const href of ['javascript:void(0)', 'https://synthetic:synthetic@example.invalid/img/1000201.png',
    'https://example.invalid/wrong/1000201.png', 'https://example.invalid/img/1000201.png?token=synthetic']) {
    await page.locator(`#p${id} .file > p > a`).evaluate((link, href) => { link.href = href; }, href);
    await trigger(page).click();
    await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveCount(0);
    await expect(page.getByRole('menuitem', { name: 'Search image on Google', exact: true })).toHaveCount(0);
    await page.keyboard.press('Escape');
  }
});
