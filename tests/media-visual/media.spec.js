import { test, expect } from '@playwright/test';

// Project-owned layout baselines, not original-site visual parity or isolation evidence.
for (const [name, viewport] of [
  ['desktop', { width: 1280, height: 900 }],
  ['mobile', { width: 390, height: 844 }],
]) {
  for (const [kind, path] of [
    ['board', '/img/'], ['thread', '/img/thread/1000201'],
    ['catalog', '/img/catalog'], ['archived', '/img/archived/1000201'],
  ]) {
    test(`attachment ${kind} ${name}`, async ({ page }) => {
      await page.setViewportSize(viewport);
      const requested = [];
      page.on('request', request => {
        if (new URL(request.url()).port === '3004') requested.push(request.url());
      });
      await page.goto(path);
      const images = page.locator(kind === 'catalog' ? '.catalogThumb img' : '.fileThumb img');
      await expect(images).toHaveCount(4);
      const expected = kind === 'catalog' ? [[150, 90], [60, 150], [50, 50], [150, 90]]
        : [[250, 150], [100, 250], [48, 32], [250, 150]];
      for (let index = 0; index < 4; index++) {
        const img = images.nth(index);
        await img.scrollIntoViewIfNeeded();
        await expect.poll(() => img.evaluate(node => node.complete && node.naturalWidth > 0)).toBe(true);
        const box = await img.boundingBox();
        expect([box.width, box.height]).toEqual(expected[index]);
        expect(await img.getAttribute('src')).toBe(`http://localhost:3004/img/${1000201 + index}${index === 3 ? '.png' : 's.jpg'}`);
        if (kind !== 'catalog') {
          // Keep desktop reference flow without squeezing narrow-screen comments.
          const float = name === 'desktop' ? 'left' : 'none';
          await expect(img).toHaveCSS('float', float);
          await expect(page.locator('.fileThumb').nth(index)).toHaveCSS('float', float);
        }
      }
      if (kind === 'catalog') {
        await expect(images.first()).toHaveAttribute('alt', `<b>fold & "roof"</b>-${'paper'.repeat(20)}.png`);
        await expect(page.locator('#thread-1000201 script, #thread-1000201 .catalogThumb b')).toHaveCount(0);
        await expect(page.locator('#thread-1000205 img, #thread-1000206 img')).toHaveCount(0);
        await expect(page.locator('#thread-1000206 .fileDeleted')).toHaveText('File deleted.');
        await expect(page.locator('#thread-1000205 .catalogThumb')).toHaveText('Spoiler image');
        const links = page.locator('.catalogThumb');
        await expect(links).toHaveCount(6);
        for (let index = 0; index < 6; index++) {
          await expect(links.nth(index)).toHaveAttribute('href', `/img/thread/${1000201 + index}`);
          await expect(page.locator('.meta').nth(index)).toHaveText('R: 0');
        }
      } else {
        await expect(page.locator('#f1000201 p a')).toHaveText(`<b>fold & "roof"</b>-${'paper'.repeat(20)}.png`);
        await expect(page.locator('.file b, .file script')).toHaveCount(0);
        await expect(page.locator('#p1000205 img, #p1000206 img, #p1000206 .file a')).toHaveCount(0);
        await expect(page.locator('#p1000206 .fileDeleted')).toHaveText('File deleted.');
        await page.getByText('Spoiler image', { exact: true }).click();
        await expect(page.getByRole('link', { name: 'View spoiler image', exact: true })).toBeVisible();
      }
      expect(requested.some(url => /100020[56]/.test(url))).toBe(false);
      expect(new Set(requested).size).toBe(4);
      for (const link of await page.locator('.file a').all()) {
        await expect(link).toHaveAttribute('rel', 'noopener noreferrer');
        await expect(link).toHaveAttribute('target', '_blank');
      }
      await expect(page.locator('#postForm')).toHaveCount(kind === 'catalog' || kind === 'archived' ? 0 : 1);
      if (kind === 'archived') await expect(page.getByText('This thread is archived and read-only.')).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
      await page.evaluate(() => window.scrollTo(0, 0));
      await page.mouse.move(0, 0);
      await expect(page).toHaveScreenshot(`media-${kind}-${name}.png`, { fullPage: true });
    });
  }
  for (const [size, teaser] of [['small','off'], ['large','off'], ['large','on']]) {
    test(`catalog ${size} teasers ${teaser} ${name}`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await page.goto(`/img/catalog?size=${size}&teaser=${teaser}`);
      const images = page.locator('.catalogThumb img');
      await expect(images).toHaveCount(4);
      await expect(page.locator('.teaser')).toHaveCount(teaser === 'on' ? 6 : 0);
      const expected = size === 'large' ? [[250,150],[100,250],[50,50],[250,150]] : [[150,90],[60,150],[50,50],[150,90]];
      for (let index = 0; index < 4; index++) {
        await images.nth(index).scrollIntoViewIfNeeded();
        await expect.poll(() => images.nth(index).evaluate(img => img.complete && img.naturalWidth > 0)).toBe(true);
        const box = await images.nth(index).boundingBox();
        expect([box.width,box.height]).toEqual(expected[index]);
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width);
      await page.evaluate(() => scrollTo(0,0));
      await page.mouse.move(0,0);
      await expect(page).toHaveScreenshot(`catalog-${size}-teasers-${teaser}-${name}.png`, { fullPage: true });
    });
  }
}
