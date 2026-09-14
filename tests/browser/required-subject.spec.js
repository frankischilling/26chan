import { test, expect } from '@playwright/test';

test('source required subjects reject cleaned-empty OPs but allow subjectless replies without JavaScript', async ({ browser }) => {
  const origin = 'http://127.0.0.1:3000', password = 'owned-required-subject-password';
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage(); let op;
  try {
    await page.goto(`${origin}/qst/`);
    // REQUIRE_SUBJECT alone does not add native HTML required in the source.
    expect(await page.locator('#sub').getAttribute('required')).toBeNull();
    await page.locator('#sub').fill('##😀'); await page.locator('#com').fill('Owned required subject');
    await page.locator('#password').fill(password);
    const denied = page.waitForResponse(response => response.url().endsWith('/qst/imgboard.php') && response.request().method() === 'POST');
    await page.getByRole('button', { name: 'Post', exact: true }).click();
    expect((await denied).status()).toBe(422);
    await expect(page.locator('body')).toContainText('Error: New threads require a subject.');
    await page.goto(`${origin}/qst/`);
    await page.locator('#sub').fill('Ｚ##ⓦ <b>'); await page.locator('#com').fill('Owned required subject');
    await page.locator('#password').fill(password); await page.getByRole('button', { name: 'Post', exact: true }).click();
    await expect(page).toHaveURL(/\/qst\/thread\/\d+#p\d+$/); op = /#p(\d+)$/.exec(page.url())[1];
    await expect(page.locator(`#pi${op} .subject`)).toHaveText('aw <b>');
    await expect(page.locator(`#pi${op} .subject b`)).toHaveCount(0);
    await page.locator('#com').fill('Owned subjectless reply'); await page.locator('#password').fill(password);
    await page.getByRole('button', { name: 'Post', exact: true }).click();
    await expect(page).toHaveURL(/\/qst\/thread\/\d+#p\d+$/);
    const reply = /#p(\d+)$/.exec(page.url())[1]; expect(reply).not.toBe(op);
    await expect(page.locator(`#m${reply}`)).toHaveText('Owned subjectless reply');
    const data = await (await context.request.get(`${origin}/qst/thread/${op}.json`)).json();
    expect(data.posts[0].sub).toBe('aw &lt;b&gt;');
    expect(data.posts.find(post => post.no === Number(reply)).sub).toBeUndefined();
  } finally {
    try { if (op) expect((await context.request.post(`${origin}/qst/delete`, {
      headers: { Origin: origin }, form: { no: op, password }, maxRedirects: 0,
    })).status()).toBe(303); } finally { await context.close(); }
  }
});
