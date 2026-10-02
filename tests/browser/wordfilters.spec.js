import { test, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { readFileSync } from 'node:fs';
import path from 'node:path';

const source = JSON.parse(readFileSync(new URL('../../fixtures/wordfilter-posting-reference.json', import.meta.url)));
const input = '[code]soy fam CUCK[/code]', origin = 'http://127.0.0.1:3000';
const replyInput = '<script>& " &#x3C; [spoiler]ordinary text[/spoiler]';
function fixture(command, slug, profile) {
  const binary = path.resolve(process.env.CARGO_TARGET_DIR || 'target', `debug/examples/wordfilter-fixture${process.platform === 'win32' ? '.exe' : ''}`);
  const result = spawnSync(binary, [command, slug, ...(profile === undefined ? [] : [String(profile)])], { encoding: 'utf8', timeout: 15_000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot } });
  expect(result.status, 'Owned wordfilter fixture must succeed').toBe(0);
}
for (const [profile, name] of [[0, 'global'], [1, 'ck'], [2, 'asp'], [3, 'v'], [4, 'test']]) {
  for (const width of [1280, 390]) {
    test(`${name} wordfilter results persist through ordinary and Quick Reply posting at width ${width}`, async ({ browser }) => {
      const slug = `z${randomBytes(5).toString('hex').slice(0, 9)}`, password = 'owned-wordfilter-password';
      fixture('setup', slug, profile);
      const context = await browser.newContext({ viewport: { width, height: 900 }, ...(width === 390 ? {
        isMobile: true, hasTouch: true,
        userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36',
      } : {}) });
      const errors = [];
      try {
        await context.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quickReply: true, persistentQR: true, threadWatcher: false, autoUpdate: false })));
        const page = await context.newPage(); page.on('pageerror', error => errors.push(error.message));
        await page.goto(`${origin}/${slug}/`);
        await page.locator(width === 390 ? '#mpostform .mobilePostFormToggle' : '#togglePostFormLink a').click();
        await page.locator('#name').fill('soy fam CUCK'); await page.locator('#sub').fill('Owned source wordfilter');
        await page.locator('#com').fill(input); await page.locator('#password').fill(password);
        await page.getByRole('button', { name: 'Post', exact: true }).click();
        await expect(page).toHaveURL(new RegExp(`/${slug}/thread/\\d+#p\\d+$`));
        const id = /#p(\d+)$/.exec(page.url())[1];
        const expected = source.profiles[name].filter(case_ => case_.input === input).map(case_ => case_.final);
        const saved = await (await context.request.get(`${origin}/${slug}/thread/${id}.json`)).json();
        expect(expected).toContain(saved.posts[0].com); expect(saved.posts[0].name).toBe('soy fam CUCK');
        const visible = await page.evaluate(html => { const document_ = new DOMParser().parseFromString(html, 'text/html'); return document_.body.textContent; }, saved.posts[0].com);
        await expect(page.locator(`#m${id}`)).toHaveText(visible);
        const header = width === 390 ? `#pim${id}` : `#pi${id}`;
        await page.locator(`${header} > .postNum > a[title="Reply to this post"]`).click();
        await page.locator('#qrCom').fill(replyInput); await page.locator('#qr-pwd').fill(password);
        await page.locator('#quickReply input[type=submit]').click();
        await expect(page.locator('.reply .postMessage')).toHaveCount(1);
        await expect(page.locator('#qrCom')).toHaveValue('');
        const completed = await (await context.request.get(`${origin}/${slug}/thread/${id}.json`)).json();
        expect(completed.posts).toHaveLength(2);
        expect(expected).toContain(completed.posts[0].com);
        const replyExpected = source.profiles[name].filter(case_ => case_.input === replyInput).map(case_ => case_.final);
        const replyTexts = await page.evaluate(html => html.map(value => new DOMParser().parseFromString(value, 'text/html').body.textContent), replyExpected);
        const replyText = await page.evaluate(html => new DOMParser().parseFromString(html, 'text/html').body.textContent, completed.posts[1].com);
        expect(replyTexts).toContain(replyText);
        expect(completed.posts[1].com).not.toMatch(/<script(?:\s|>)/i);
        await expect(page.locator('.postMessage script')).toHaveCount(0);
        for (let attempt = 0; attempt < 3; attempt++) {
          await page.reload();
          const current = await (await context.request.get(`${origin}/${slug}/thread/${id}.json`)).json();
          expect(current.posts.map(post => post.com)).toEqual(completed.posts.map(post => post.com));
          for (const post of current.posts) {
            const text = await page.evaluate(html => new DOMParser().parseFromString(html, 'text/html').body.textContent, post.com);
            await expect(page.locator(`#m${post.no}`)).toHaveText(text);
          }
        }
        expect(errors).toEqual([]);
      } finally {
        try { await context.close(); } finally { fixture('cleanup', slug); }
      }
    });
  }
}
