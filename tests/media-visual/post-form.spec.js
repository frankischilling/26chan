import { test, expect } from '@playwright/test';
test.use({ javaScriptEnabled: true });

test('desktop reveal is one-way, preserves drafts and the source reply hash opens on load', async ({ page }) => {
  await page.goto('/demo/');
  await expect(page.locator('#postForm')).toBeHidden();
  await page.locator('#togglePostFormLink a').click();
  await expect(page.locator('#postForm')).toBeVisible();
  await expect(page.locator('#togglePostFormLink')).toBeHidden();
  await page.locator('#com').fill('Ordinary retained draft');
  await page.locator('.postInfo > .postNum').first().click();
  await page.locator('#qrCom').fill('Separate Quick Reply draft');
  await page.locator('#qrCom').press('Escape');
  await expect(page.locator('#com')).toHaveValue('Ordinary retained draft');
  await page.goto('/demo/#reply');
  await expect(page.locator('#postForm')).toBeVisible();
  await expect(page.locator('#togglePostFormLink')).toBeHidden();
});

test('mobile top and bottom toggle the ordinary board form without clearing it', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 900 }); await page.goto('/demo/');
  const top = page.locator('#mpostform a'), bottom = page.locator('.postFormBottom a');
  await expect(page.locator('#postForm')).toBeHidden();
  await top.click(); await expect(page.locator('#postForm')).toBeVisible();
  await expect(top).toHaveText('Close Post Form'); await expect(top).toHaveAttribute('aria-expanded', 'true');
  await page.locator('#com').fill('Retained mobile draft');
  await bottom.click(); await expect(page.locator('#postForm')).toBeHidden();
  await expect(top).toHaveText('Start New Thread');
  expect(await top.evaluate(node => node.getBoundingClientRect().top >= 0 && node.getBoundingClientRect().top < innerHeight)).toBe(true);
  await top.click(); await expect(page.locator('#com')).toHaveValue('Retained mobile draft');
});

test('mobile thread entry uses enabled Quick Reply without quoting, or toggles the native form', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 900 }); await page.goto('/img/thread/1000201');
  await page.locator('h1').evaluate(node => { const range = document.createRange(); range.selectNodeContents(node); getSelection().removeAllRanges(); getSelection().addRange(range); });
  await page.locator('#mpostform a').click(); await expect(page.locator('#quickReply')).toBeVisible();
  await expect(page.locator('#qrCom')).toHaveValue(''); await expect(page.locator('#postForm')).toBeHidden();
  await page.locator('#qrCom').fill('Mobile QR draft'); await page.locator('.postFormBottom a').click();
  await expect(page.locator('#qrCom')).toHaveValue('Mobile QR draft');
  await page.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: false }))); await page.reload();
  await page.locator('#mpostform a').click(); await expect(page.locator('#postForm')).toBeVisible();
  await expect(page.locator('#quickReply')).toHaveCount(0);
  await page.locator('#com').fill('Ordinary reply'); await page.locator('#mpostform a').click();
  await expect(page.locator('#mpostform a')).toHaveText('Post Reply');
  await page.locator('.postFormBottom a').click(); await expect(page.locator('#com')).toHaveValue('Ordinary reply');
});

test('core form controls work with the extension disabled and preserve source viewport states', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
  await page.goto('/demo/#reply'); await expect(page.locator('#postForm')).toBeVisible();
  await page.setViewportSize({ width: 390, height: 900 }); await expect(page.locator('#postForm')).toBeHidden();
  await page.locator('#mpostform a').click(); await expect(page.locator('#postForm')).toBeVisible();
  await page.setViewportSize({ width: 1280, height: 900 }); await expect(page.locator('#postForm')).toBeVisible();
  await page.goto('/img/closed/1000201'); await expect(page.locator('#togglePostFormLink, .mobilePostFormToggle')).toHaveCount(0);
});
