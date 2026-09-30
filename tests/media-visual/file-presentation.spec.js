import { test, expect } from '@playwright/test';
test.use({ javaScriptEnabled: true });

const path = '/img/thread/1000201';
const full = `<b>fold & "roof"</b>-${'paper'.repeat(20)}.png`;
test('file headers retain escaped full labels, mobile captions, tooltip timing and native menu targets', async ({ page }) => {
  await page.goto(path);
  const header = page.locator('#fT1000201'), label = header.locator('a'), caption = page.locator('#f1000201 .mFileInfo');
  await expect(label).toHaveText(full.slice(0, 35) + '(...).png');
  await expect(label).toHaveAttribute('title', full);
  await expect(label).toHaveAttribute('href', 'http://localhost:3004/img/1000201.png');
  await expect(header).toContainText('600x360');
  await expect(caption).toBeHidden();
  await expect(page.locator('.file b,.file script')).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(header).toBeHidden(); await expect(caption).toBeVisible();
  await page.clock.install(); await page.clock.pauseAt(new Date(Date.now() + 1000));
  await caption.dispatchEvent('mouseover'); await page.clock.fastForward(299);
  await expect(page.locator('#tooltip')).toHaveCount(0);
  await page.clock.fastForward(1); await expect(page.locator('#tooltip')).toHaveText(full);
  await expect(page.locator('#tooltip img,#tooltip b')).toHaveCount(0);
  await caption.dispatchEvent('mouseout'); await expect(page.locator('#tooltip')).toHaveCount(0);
  await expect(caption).not.toHaveAttribute('aria-describedby');
  await page.getByRole('button', { name: 'Post menu for post 1000201', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: 'Open normalized file', exact: true })).toHaveAttribute('href', 'http://localhost:3004/img/1000201.png');
  await page.keyboard.press('Escape');
  await page.clock.resume();
});

test('filename filters use the full uploaded label while forged or copied captions cannot trigger callbacks', async ({ page }) => {
  await page.goto('/img/');
  await page.evaluate(full => {
    localStorage.setItem('4chan-settings', JSON.stringify({ filter: true }));
    localStorage.setItem('4chan-filters', JSON.stringify([{ type: 6, pattern: `"${full}"`, boards: '', active: true, hide: true }]));
  }, full);
  await page.reload();
  await expect(page.locator('#t1000201')).toHaveClass(/post-hidden/);
  await page.evaluate(() => localStorage.removeItem('4chan-filters'));
  await page.reload(); await page.setViewportSize({ width: 390, height: 844 });
  const caption = page.locator('#f1000201 .mFileInfo');
  await page.evaluate(() => {
    window.ownedCallbackCalls = 0; window.ownedUnexpectedCallback = () => { window.ownedCallbackCalls++; };
    document.querySelector('#f1000201 .mFileInfo').setAttribute('data-tip-cb', 'ownedUnexpectedCallback');
    document.querySelector('#fT1000201 a').title = 'Forged filename.png';
  });
  await page.clock.install(); await caption.dispatchEvent('mouseover'); await page.clock.fastForward(1000);
  await expect(page.locator('#tooltip')).toHaveCount(0);
  expect(await page.evaluate(() => window.ownedCallbackCalls)).toBe(0);
  await page.evaluate(async full => {
    document.querySelector('#fT1000201 a').title = full;
    document.querySelector('#f1000201 .mFileInfo').removeAttribute('data-tip-cb');
    const { localQuoteTree, prepareQuotePost } = await import('/static/native-filter.v1.js');
    const context = { origin: location.origin, mediaOrigin: 'http://localhost:3004', board: 'img', thread: '1000201' };
    const tree = localQuoteTree(document.getElementById('pc1000201'), context, '1000201');
    const copy = document.createElement('div'); copy.id = 'owned-file-copy';
    copy.append(prepareQuotePost(tree, context, '1000201').build(document));
    document.querySelector('.board').append(copy);
  }, full);
  await caption.dispatchEvent('mouseout');
  await page.locator('#owned-file-copy .mFileInfo').dispatchEvent('mouseover'); await page.clock.fastForward(1000);
  await expect(page.locator('#tooltip')).toHaveCount(0);
  await page.locator('#owned-file-copy').evaluate(copy => copy.remove());
  await caption.dispatchEvent('mouseover'); await page.clock.fastForward(300);
  await expect(page.locator('#tooltip')).toHaveText(full);
  await page.evaluate(() => document.getElementById('pc1000201').remove());
  await expect(page.locator('#tooltip')).toHaveCount(0);
});

test('the 480px boundary, exact mobile opt-out and picture preference preserve the same original file', async ({ page, context }) => {
  await page.setViewportSize({ width: 481, height: 844 }); await page.goto(path);
  const header = page.locator('#fT1000201'), caption = page.locator('#f1000201 .mFileInfo');
  const image = page.locator('#f1000201 .fileThumb > img');
  await expect(header).toBeVisible(); await expect(caption).toBeHidden();
  await expect(image).toHaveCSS('max-width', 'none');
  await page.setViewportSize({ width: 480, height: 844 });
  await expect(header).toBeHidden(); await expect(caption).toBeVisible();
  await expect(image).toHaveCSS('max-width', '125px');
  const other = await context.newPage();
  try {
    await other.goto(path);
    await other.evaluate(() => localStorage.setItem('4chan_never_show_mobile', 'true'));
    await expect(header).toBeVisible(); await expect(caption).toBeHidden();
    await expect(image).toHaveCSS('max-width', 'none');
    await other.evaluate(() => localStorage.setItem('4chan_never_show_mobile', '1'));
    await expect(header).toBeHidden(); await expect(caption).toBeVisible();
    await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ noPictures: true })));
    await expect(image).toHaveCSS('opacity', '0'); await expect(caption).toBeHidden();
    await other.evaluate(() => localStorage.removeItem('4chan-settings'));
    await expect(image).toHaveCSS('opacity', '1'); await expect(caption).toBeVisible();
    await expect(image).toHaveAttribute('src', 'http://localhost:3004/img/1000201s.jpg');
  } finally { await other.close(); }
});
