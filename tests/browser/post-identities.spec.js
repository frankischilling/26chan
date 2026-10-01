import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000', password = 'owned-identity-browser-password';

test('persisted tripcodes survive browser previews, filters and ordinary rendering', async ({ page, context, request }) => {
  const threads = [];
  const post = async (parent, name, com) => {
    const response = await request.post('/demo/post', { headers: { Origin: origin }, maxRedirects: 0,
      form: { resto: parent, name, sub: 'Owned identity', com, password } });
    expect(response.status()).toBe(303);
    const id = response.headers().location.match(/#p(\d+)$/)[1];
    if (parent === '0') threads.push(id);
    return id;
  };
  try {
    const target = await post('0', '<owned name>#password', 'Owned tripcode target');
    const reply = await post(target, 'Reply#password', 'Owned reply with the same pseudonym');
    const remote = await post('0', 'Plain name', `>>${target}\nOwned remote reference`);
    await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quotePreview: true, threadStats: false, filter: true })));
    await page.goto(`/demo/thread/${remote}`);
    await page.locator(`#m${remote} a.quotelink`).hover();
    const preview = page.locator('#quote-preview');
    await expect(preview).toBeVisible();
    await expect(preview.locator('.postInfo .postertrip')).toHaveText('!ozOtJW9BFA');
    await expect(preview.locator('.postInfo .name')).toHaveText('<owned name>');
    await expect(preview.locator('owned, script, form, input')).toHaveCount(0);
    await page.goto(`/demo/thread/${target}`);
    await expect(page.locator(`#pi${target} .postertrip`)).toHaveText('!ozOtJW9BFA');
    await expect(page.locator(`#pi${reply} .postertrip`)).toHaveText('!ozOtJW9BFA');
    for (const theme of ['yotsuba', 'yotsuba-b', 'futaba', 'burichan', 'tomorrow', 'photon']) {
      await context.addCookies([{ name: 'board-theme-ws', value: theme, url: origin, httpOnly: true, sameSite: 'Lax' }]);
      await page.reload();
      const style = await page.locator(`#pi${reply} .postertrip`).evaluate(node => {
        const actual = getComputedStyle(node), name = getComputedStyle(node.parentElement.querySelector('.name'));
        return { weight: actual.fontWeight, sameColor: actual.color === name.color };
      });
      expect(style, theme).toEqual({ weight: '400', sameColor: true });
    }
    await page.evaluate(() => localStorage.setItem('4chan-filters', JSON.stringify([
      { type: 0, pattern: '!ozOtJW9BFA', boards: '', active: true, auto: false, hide: true },
    ])));
    await page.reload();
    await expect(page.locator(`#p${reply}`)).toHaveClass(/post-hidden/);
    await expect(page.locator(`#m${reply}`)).toBeHidden();
    await expect(page.locator(`#pi${target} .postertrip`)).toBeVisible();
    const data = await (await request.get(`/demo/thread/${target}.json`)).json();
    expect(data.posts.map(post => post.trip)).toEqual(['!ozOtJW9BFA', '!ozOtJW9BFA']);
    expect(data.posts.map(post => post.name)).toEqual(['<owned name>', 'Reply']);
  } finally {
    for (const thread of threads.reverse()) {
      expect((await request.post('/demo/delete', { headers: { Origin: origin }, maxRedirects: 0,
        form: { no: thread, password } })).status()).toBe(303);
    }
  }
});
