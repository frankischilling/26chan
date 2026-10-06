import { ownedDeletionMarker, cleanupDeletionFixtures } from './helpers/deletion-fixture.js';
import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';

const origin = 'http://127.0.0.1:3000';
for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test(`board dice and fortunes remain stable across public projections at ${viewport.width}px`, async ({ browser }, testInfo) => {
    const mobile = viewport.width === 390;
    const context = await browser.newContext({ viewport, ...(mobile ? {
      isMobile: true, hasTouch: true,
      userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36',
    } : {}) });
    const page = await context.newPage();
    const marker = ownedDeletionMarker();
    const password = `delete-${randomUUID()}`;
    const threads = [];
    const violations = [];
    page.on('pageerror', error => violations.push(error.message));
    await page.exposeFunction('recordRandomizerViolation', value => violations.push(value));
    await page.addInitScript(() => {
      window.randomizerPolicyViolations = [];
      document.addEventListener('securitypolicyviolation', event => {
        const violation = `${event.effectiveDirective}: ${event.blockedURI}`;
        randomizerPolicyViolations.push(violation);
        window.recordRandomizerViolation(violation);
      });
    });
    async function post(board, options, comment = marker, parent = '0') {
      const response = await context.request.post(`${origin}/${board}/imgboard.php`, {
        headers: { Origin: origin, Accept: 'application/json' },
        form: { mode: 'regist', resto: parent, sub: marker, com: comment, pwd: password, email: options },
      });
      expect(response.status(), await response.text()).toBe(200);
      const id = String((await response.json()).pid);
      if (parent === '0') threads.push({ board, id });
      return id;
    }
    try {
      const dice = await post('tg', 'dice+2d1+3');
      const fortune = await post('b', 'SaGefortunesage');
      const plain = await post('fixture', 'dice+0d0', `${marker}\n>>>/tg/${dice}\n>>>/b/${fortune}`);
      const expected = 'Rolled 1, 1 + 3 = 5 (2d1 + 3)';
      await page.goto(`/tg/thread/${dice}`);
      await expect(page.locator(`#m${dice} > b`)).toHaveText(expected);
      expect((await (await context.request.get(`/tg/thread/${dice}.json`)).json()).posts[0].com.startsWith(`<b>${expected}<br><br></b>`)).toBe(true);
      await page.reload();
      await expect(page.locator(`#m${dice} > b`)).toHaveText(expected);
      const reply = await post('tg', 'dice+1d1', `>>${dice}\n${marker}`, dice);
      await page.locator('a[data-cmd="update"]:visible').first().click();
      await expect(page.locator(`#m${reply} > b`)).toHaveText('Rolled 1 (1d1)');
      await expect(page.locator(`#m${reply} .quotelink`)).toHaveAttribute('href', `/tg/post/${dice}`);
      const feed = await context.request.get(`${origin}/tg/index.rss`);
      expect(feed.status()).toBe(200);
      const description = await page.evaluate(({ xml, guid }) => {
        const document = new DOMParser().parseFromString(xml, 'application/xml');
        if (document.querySelector('parsererror')) throw new Error('Malformed RSS');
        return [...document.querySelectorAll('item')].find(item => item.querySelector('guid')?.textContent === guid)?.querySelector('description')?.textContent;
      }, { xml: await feed.text(), guid: `${origin}/tg/thread/${dice}` });
      expect(description).toContain(`<b>${expected}<br><br></b>`);
      await page.goto('/tg/catalog');
      const teaser = page.locator(`#thread-${dice} .teaser`);
      await expect(teaser).toHaveText(`${marker}: ${expected} ${marker}`);
      expect(await teaser.locator('b').count()).toBe(1);
      await page.goto(`/globalsearch.php#/${marker}/tg`);
      await expect(page.locator(`#m${dice} > b`)).toHaveText(expected);

      await page.goto(`/b/thread/${fortune}`);
      const fortuneNode = page.locator(`#m${fortune} > .fortune`);
      await expect(fortuneNode).toBeVisible();
      const text = await fortuneNode.textContent();
      const color = await fortuneNode.evaluate(node => getComputedStyle(node).color);
      const json = (await (await context.request.get(`/b/thread/${fortune}.json`)).json()).posts[0].com;
      const hex = json.match(/style="color:(#[0-9a-f]{6})"/)[1];
      expect(color).toBe(`rgb(${[1, 3, 5].map(index => Number.parseInt(hex.slice(index, index + 2), 16)).join(', ')})`);
      expect(json).toContain(text);
      await page.reload();
      await expect(fortuneNode).toHaveText(text);
      expect(await fortuneNode.evaluate(node => getComputedStyle(node).color)).toBe(color);
      await page.screenshot({ path: testInfo.outputPath('fortune.png'), fullPage: true });
      await page.goto(`/fixture/thread/${plain}`);
      await expect(page.locator(`#m${plain} > b, #m${plain} > .fortune`)).toHaveCount(0);
      for (const [board, id, generated] of [['tg', dice, expected], ['b', fortune, text]]) {
        await page.goto(`/fixture/thread/${plain}`);
        const link = page.locator(`#m${plain} .quotelink[href="/${board}/post/${id}"]`);
        if (mobile) await link.tap(); else await link.hover();
        const preview = page.locator('#quote-preview .postMessage');
        await expect(preview).toContainText(generated);
        if (board === 'b') expect(await preview.locator('.fortune').evaluate(node => getComputedStyle(node).color)).toBe(color);
        await expect(page.locator('#quote-preview [id], #quote-preview script, #quote-preview form')).toHaveCount(0);
      }
      expect(await page.evaluate(() => randomizerPolicyViolations)).toEqual([]);
      expect(violations).toEqual([]);
    } finally {
      try {
        try {
          cleanupDeletionFixtures(threads.filter(({ board }) => board !== 'fixture').map(entry => ({ ...entry, marker })));
        } finally {
          for (const { board, id } of threads.filter(({ board }) => board === 'fixture')) {
            expect((await context.request.post(`${origin}/${board}/delete`, {
              headers: { Origin: origin }, form: { no: id, password }, maxRedirects: 0,
            })).status()).toBe(303);
          }
        }
      } finally { await context.close(); }
    }
  });
}
