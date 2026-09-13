import { test, expect } from '@playwright/test';

test.use({ javaScriptEnabled: true });

const themes = ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon'];
const watches = {
  '1001001-demo': ['Read thread', 1001001, 0, false, false],
  '1001002-demo': ['Unread thread', 1001002, 2, false, false],
  '1001003-demo': ['Archived thread', 1001003, 0, true, false],
  '1001004-demo': ['Own reply thread', 1001004, 3, true, true],
  '1001005-demo': ['Dead thread', -1, 6, true, true],
  '1001006-demo': ['<img src=x onerror=alert(1)>', 0, 0, false, false],
};

for (const theme of themes) {
  test(`watcher row states match the pinned client in ${theme} on desktop and mobile`, async ({ page, context }) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    await context.addInitScript(watches => {
      localStorage.setItem('4chan-settings', JSON.stringify({ threadWatcher: true }));
      localStorage.setItem('4chan-watch', JSON.stringify(watches));
      // Presentation fixtures must not race a refresh of these synthetic IDs.
      localStorage.setItem('4chan-tw-timestamp', String(Date.now()));
    }, watches);
    for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
      await page.setViewportSize(viewport);
      for (const catalog of [true, false]) {
        await page.goto(catalog ? '/demo/catalog' : '/demo/');
        const fold = page.locator('.watcherFold');
        if (viewport.width === 390) await fold.click();
        const links = page.locator('#watchList a');
        await expect(links).toHaveText([
          '/demo/ - Read thread', '(2) /demo/ - Unread thread',
          '/demo/ - Archived thread', '(3) /demo/ - Own reply thread',
          '/demo/ - Dead thread', '/demo/ - <img src=x onerror=alert(1)>',
        ]);
        await expect(page.locator('#watchList img, #watchList script')).toHaveCount(0);
        const normal = links.nth(0);
        await expect(normal).toHaveCSS('font-weight', '400');
        await expect(normal).toHaveCSS('font-style', 'normal');
        await expect(normal).toHaveCSS('opacity', '1');
        await expect(normal).toHaveAttribute('href', `/demo/thread/1001001#${catalog ? 'p' : 'lr'}1001001`);
        await expect(links.nth(1)).toHaveClass('hasNewReplies');
        await expect(links.nth(1)).toHaveCSS('font-weight', '700');
        await expect(links.nth(2)).toHaveClass('archivelink');
        await expect(links.nth(2)).toHaveCSS('opacity', '0.5');
        const own = links.nth(3);
        await expect(own).toHaveClass(/hasNewReplies/);
        await expect(own).toHaveClass(/hasYouReplies/);
        await expect(own).toHaveClass(/archivelink/);
        await expect(own).toHaveCSS('font-weight', '700');
        await expect(own).toHaveCSS('font-style', 'italic');
        await expect(own).toHaveCSS('opacity', '0.5');
        await expect(own).toHaveAttribute('title', 'This thread has replies to your posts');
        const dead = links.nth(4);
        await expect(dead).toHaveClass('deadlink');
        await expect(dead).toHaveCSS('text-decoration-line', 'line-through');
        await expect(dead).toHaveCSS('font-weight', '400');
        await expect(dead).toHaveCSS('font-style', 'normal');
        await expect(dead).toHaveCSS('opacity', '1');
        await expect(dead).not.toHaveAttribute('title');
        await expect(dead).toHaveAttribute('href', `/demo/thread/1001005${catalog ? '' : '#lr-1'}`);
        await expect(links.nth(5)).toHaveAttribute('href', `/demo/thread/1001006${catalog ? '' : '#lr0'}`);
        const remove = page.getByRole('button', { name: 'Unwatch /demo/ thread 1001001', exact: true });
        await expect(remove).toHaveText('\u00d7');
        await expect(remove).toHaveCSS('border-top-width', '0px');
        await remove.focus();
        await remove.press('Enter');
        await expect(page.locator('#watch-1001001-demo')).toHaveCount(0);
        await expect(links).toHaveCount(5);
      }
    }
  });
}
