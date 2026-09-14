import { test, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import path from 'node:path';

function fixture(command, slug) {
  const extension = process.platform === 'win32' ? '.exe' : '';
  const binary = path.resolve(process.env.CARGO_TARGET_DIR || 'target', `debug/examples/forced-anonymous-fixture${extension}`);
  const result = spawnSync(binary, [command, slug], { encoding: 'utf8', timeout: 15_000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot } });
  expect(result.status, 'Owned forced-anonymous fixture must succeed').toBe(0);
}
for (const javaScriptEnabled of [false, true]) {
  test(`forced-anonymous native and Quick Reply forms preserve anonymous posting (JavaScript ${javaScriptEnabled})`, async ({ browser }, info) => {
    const slug = `z${randomBytes(5).toString('hex').slice(0, 9)}`, password = 'owned-anonymous-password';
    const origin = 'http://127.0.0.1:3000'; fixture('setup', slug);
    const context = await browser.newContext({ javaScriptEnabled });
    try {
      const page = await context.newPage();
      await page.goto(`${origin}/${slug}/`);
      if (javaScriptEnabled) await page.locator('#togglePostFormLink a').click();
      await expect(page.locator('#name, #sub')).toHaveCount(0);
      await expect(page.locator('form.postEditor input[name=name]')).toHaveAttribute('type', 'hidden');
      await page.screenshot({ path: info.outputPath('forced-anonymous-form.png'), fullPage: true });
      await page.locator('#com').fill('Owned anonymous OP'); await page.locator('#password').fill(password);
      await page.getByRole('button', { name: 'Post', exact: true }).click();
      await expect(page).toHaveURL(new RegExp(`/${slug}/thread/\\d+#p\\d+$`));
      const op = /#p(\d+)$/.exec(page.url())[1];
      await expect(page.locator(`#pi${op} .name`)).toHaveText('Anonymous');
      if (javaScriptEnabled) {
        await page.locator(`#p${op} .postInfo > .postNum`).click();
        await expect(page.locator('#qr-name')).toHaveCount(0);
        await expect(page.locator('#quickReply input[name=name]')).toHaveAttribute('type', 'hidden');
        await page.locator('#qrCom').fill('Owned anonymous reply'); await page.locator('#qr-pwd').fill(password);
        const url = page.url(); await page.locator('#quickReply input[type=submit]').click();
        await expect(page.locator('.reply .postMessage')).toHaveText('Owned anonymous reply'); expect(page.url()).toBe(url);
      } else {
        await page.locator('#com').fill('Owned anonymous reply'); await page.locator('#password').fill(password);
        await page.getByRole('button', { name: 'Post', exact: true }).click();
      }
      await expect(page.locator('.reply .name')).toHaveText('Anonymous');
      await page.reload(); await expect(page.locator('.reply .name')).toHaveText('Anonymous');
      const json = await (await context.request.get(`${origin}/${slug}/thread/${op}.json`)).json();
      expect(json.posts).toHaveLength(2);
      for (const post of json.posts) { expect(post.name).toBe('Anonymous'); expect(post.sub).toBeUndefined(); }
    } finally { await context.close(); fixture('cleanup', slug); }
  });
}
