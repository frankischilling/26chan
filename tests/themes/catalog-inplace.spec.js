import { test, expect, attachComparedImage } from '../helpers/visual-diagnostics.js';
import { readFile } from 'node:fs/promises';

// Only this interactive suite overrides the inherited static, no-JavaScript mode.
test.use({ javaScriptEnabled: true });

const reference = JSON.parse(await readFile(new URL('../../docs/public-catalog-reference.json', import.meta.url), 'utf8'));
for (const theme of Object.keys(reference.themes)) {
  test(`in-place catalog dimensions and pixels match server modes in ${theme}`, async ({ context }, info) => {
    await context.addCookies([{ name: 'board-theme-ws', value: theme, url: 'http://127.0.0.1:3000', httpOnly: true, sameSite: 'Lax' }]);
    const live = await context.newPage();
    const server = await context.newPage();
    for (const [width, height] of reference.environment.viewports) {
      await live.setViewportSize({ width, height });
      await server.setViewportSize({ width, height });
      await live.goto('/img/catalog?order=alt&size=small&teaser=off');
      let navigations = 0;
      const listener = request => { if (request.isNavigationRequest() && request.frame() === live.mainFrame()) navigations += 1; };
      live.on('request', listener);
      for (const [size, teaser] of [['large','on'], ['large','off'], ['small','on'], ['small','off']]) {
        await live.locator('#size-ctrl').selectOption(size);
        await live.locator('#teaser-ctrl').selectOption(teaser);
        await server.goto(`/img/catalog?order=alt&size=${size}&teaser=${teaser}`);
        for (const page of [live, server]) {
          await page.locator('#threads img').evaluateAll(nodes => Promise.all(nodes.map(image => image.decode())));
        }
        const attrs = page => page.locator('#threads img').evaluateAll(nodes => nodes.map(image => [image.getAttribute('src'), image.width, image.height]));
        expect(await attrs(live)).toEqual(await attrs(server));
        const liveImage = await live.locator('#threads').screenshot({ animations: 'disabled' });
        const serverImage = await server.locator('#threads').screenshot({ animations: 'disabled' });
        if (!liveImage.equals(serverImage)) {
          await attachComparedImage(info, `live-${theme}-${width}-${size}-${teaser}`, liveImage);
          await attachComparedImage(info, `server-${theme}-${width}-${size}-${teaser}`, serverImage);
        }
        expect(liveImage).toEqual(serverImage);
        expect(navigations).toBe(0);
      }
      live.off('request', listener);
    }
  });
}
