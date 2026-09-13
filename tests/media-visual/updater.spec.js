import { test, expect } from '@playwright/test';
test.use({ javaScriptEnabled: true });
const origin = 'http://127.0.0.1:3000';

test('updater retains owned normal, spoiler and deleted-file rendering and native image menus', async ({ browser, page }, info) => {
  const plain = await browser.newContext({ javaScriptEnabled: false });
  const source = await plain.newPage(); await source.goto(`${origin}/img/thread/1000201`);
  const posts = await source.locator('.postContainer').evaluateAll(nodes => nodes.map(node => ({ no: node.id.slice(2),
    file_deleted: !!node.querySelector('.fileDeleted'), html: node.outerHTML })));
  await plain.close();
  const snapshot = { version: 1, board: 'img', thread: '1000201', closed: false, archived: false, sticky: false,
    replies: posts.length - 1, images: 4, posts };
  await page.goto('/img/thread/1000201');
  await page.locator('.replyContainer').evaluateAll(nodes => nodes.forEach(node => node.remove()));
  await expect(page.locator('.postContainer')).toHaveCount(1);
  await page.route('**/_watch/img/thread/1000201/posts', route => route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) }));
  await page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
  await expect(page.locator('.threadNav.desktop .nativeUpdaterStatus').first()).toHaveText('5 new posts');
  await expect(page.locator('.postContainer')).toHaveCount(6);
  await expect(page.locator('#f1000202 img')).toHaveAttribute('src', 'http://localhost:3004/img/1000202s.jpg');
  await expect(page.locator('#f1000205 img, #p1000206 img')).toHaveCount(0);
  await expect(page.locator('#p1000206 .fileDeleted')).toHaveText('File deleted.');
  await page.locator('#f1000205 summary').click();
  await expect(page.getByRole('link', { name: 'View spoiler image', exact: true })).toHaveAttribute('href', 'http://localhost:3004/img/1000205.png');
  await page.setViewportSize({ width: 390, height: 900 });
  await page.getByRole('button', { name: 'Post menu for post 1000202', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveAttribute('href', 'http://localhost:3004/img/1000202.png');
  await page.keyboard.press('Escape');
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    for (const image of await page.locator('.fileThumb img').all()) {
      await image.scrollIntoViewIfNeeded();
      await expect.poll(() => image.evaluate(node => node.complete && node.naturalWidth > 0)).toBe(true);
    }
    await page.evaluate(() => scrollTo(0, 0));
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    const path = info.outputPath(`updater-media-${width}.png`); await page.screenshot({ path, fullPage: true });
    await info.attach(`updated media ${width}`, { path, contentType: 'image/png' });
  }
});
