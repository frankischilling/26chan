import { test, expect } from '@playwright/test';

test('persisted catalog teasers use board policy in HTML, GET filtering and live search', async ({ browser }, info) => {
  const origin = 'http://127.0.0.1:3000', password = 'owned-catalog-teaser-password';
  const title = `Teaser${Date.now()}`;
  const cases = [
    { board: 'b', comment: 'x'.repeat(301), teaser: `${'x'.repeat(300)}…`, query: 'x…$' },
    { board: 'b', comment: 'y'.repeat(40), teaser: `${'y'.repeat(35)}<wbr>${'y'.repeat(5)}`, query: '<wbr>yyyyy$' },
    { board: 'b', comment: '[spoiler] [/spoiler]', teaser: '', query: '</b>$' },
    { board: 'sjis', comment: '[sjis]wide  art\nnext[/sjis]after [spoiler]quiet[/spoiler] <script>owned</script>', teaser: '[SJIS]after <s>quiet</s> &lt;script&gt;owned&lt;/script&gt;', query: '[SJIS]after' },
    { board: 'news', comment: 'first\n\nsecond', teaser: 'first\nsecond', query: 'second$' },
  ];
  const noScript = await browser.newContext({ javaScriptEnabled: false });
  const liveContext = await browser.newContext();
  const server = await noScript.newPage(), live = await liveContext.newPage();
  const created = [];
  try {
    for (const entry of cases) {
      await server.goto(`${origin}/${entry.board}/`);
      await server.locator('#sub').fill(title);
      await server.locator('#com').fill(entry.comment);
      await server.locator('#password').fill(password);
      await server.getByRole('button', { name: 'Post', exact: true }).click();
      await expect(server).toHaveURL(/\/thread\/\d+#p\d+$/);
      entry.id = /#p(\d+)$/.exec(server.url())[1];
      created.push(entry);
      await server.goto(`${origin}/${entry.board}/catalog?q=${encodeURIComponent(entry.query)}`);
      const card = server.locator(`#threads #thread-${entry.id}`);
      await expect(card).toBeVisible();
      await expect(card.locator('.catalogThumb')).toHaveAttribute('data-search-text', `<b>${title}</b>${entry.teaser ? `: ${entry.teaser}` : ''}`);
      if (!entry.teaser) await expect(card.locator('.teaser')).toHaveText(title);
      if (entry.board === 'b') await expect(card.locator('wbr')).toHaveCount(entry.comment.length === 40 ? 1 : 0);
      if (entry.board === 'news') {
        expect(await card.locator('template.catalogTeaser').evaluate(node => node.content.querySelector('.teaser').textContent)).toBe(`${title}: first\nsecond`);
        await expect(card.locator('.txt-sub > a')).toHaveText(title);
        await expect(card.locator('.teaser')).toHaveCount(0);
      }
      if (entry.board === 'sjis') {
        await expect(card.locator('s')).toHaveText('quiet');
        await expect(card.locator('.sjis, script')).toHaveCount(0);
        await expect(card.locator('.teaser')).toContainText('[SJIS]after quiet <script>owned</script>');
      }
      await live.goto(`${origin}/${entry.board}/catalog?q=${title}`);
      let navigations = 0;
      live.on('request', request => { if (request.isNavigationRequest()) navigations += 1; });
      await live.locator('#qf-box').fill(entry.query);
      await live.getByRole('button', { name: 'Apply', exact: true }).click();
      await expect(live.locator(`#threads #thread-${entry.id}`)).toBeVisible();
      await live.locator('#qf-box').fill('^absent-owned-teaser$');
      await live.getByRole('button', { name: 'Apply', exact: true }).click();
      await expect(live.locator('#threads .thread')).toHaveCount(0);
      await live.locator('#qf-box').fill(entry.query);
      await live.getByRole('button', { name: 'Apply', exact: true }).click();
      await expect(live.locator(`#threads #thread-${entry.id}`)).toBeVisible();
      expect(navigations).toBe(0);
      live.removeAllListeners('request');
      for (const width of [1280, 390]) {
        await server.setViewportSize({ width, height: 900 });
        const path = info.outputPath(`teaser-${entry.board}-${entry.id}-${width}.png`);
        await card.screenshot({ path });
        await info.attach(`teaser ${entry.board} ${width}`, { path, contentType: 'image/png' });
      }
    }
  } finally {
    try {
      for (const { board, id } of created) {
        const response = await noScript.request.post(`${origin}/${board}/delete`, { headers: { origin }, form: { no: id, password }, maxRedirects: 0 });
        expect(response.status()).toBe(303);
      }
    } finally { await noScript.close(); await liveContext.close(); }
  }
});
