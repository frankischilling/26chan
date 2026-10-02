import { test, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
for (const javaScriptEnabled of [false, true]) {
  test(`board and catalog links survive posting and updating (JavaScript ${javaScriptEnabled})`, async ({ browser }) => {
    const context = await browser.newContext({ javaScriptEnabled });
    const password = 'owned-static-quotes-password';
    let op;
    try {
      const response = await context.request.post(`${origin}/fixture/post`, { headers: { origin, accept: 'application/json' },
        form: { resto: '0', sub: 'Owned static references', com: '>>>/po/ >>>/g/catalog >>>/g/a+b/c,d-e >>>/unknown/catalog', pwd: password } });
      expect(response.status()).toBe(200);
      op = String((await response.json()).pid);
      const page = await context.newPage();
      await page.goto(`${origin}/fixture/thread/${op}`);
      const message = page.locator(`#m${op}`);
      for (const [label, href] of [['>>>/po/', '/po/'], ['>>>/g/catalog', '/g/catalog'], ['>>>/g/a+b/c,d-e', '/g/catalog#s=a+b%2Fc%2Cd-e']]) {
        await expect(message.getByRole('link', { name: label, exact: true })).toHaveAttribute('href', href);
      }
      await expect(message.getByRole('link', { name: '>>>/unknown/catalog', exact: true })).toHaveCount(0);
      const json = await (await context.request.get(`${origin}/fixture/thread/${op}.json`)).json();
      expect(json.posts[0].com).toContain('href="/g/catalog#s=a+b%2Fc%2Cd-e"');
      expect(json.posts[0].com).not.toContain('href="/unknown/');
      if (javaScriptEnabled) {
        await page.locator('.threadNav.desktop input[data-cmd="auto"]').first().uncheck();
        const reply = await context.request.post(`${origin}/fixture/post`, { headers: { origin, accept: 'application/json' },
          form: { resto: op, com: '>>>/g/catalog', pwd: password } });
        expect(reply.status()).toBe(200);
        const id = String((await reply.json()).pid);
        await page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
        await expect(page.locator(`#m${id}`).getByRole('link', { name: '>>>/g/catalog', exact: true })).toHaveAttribute('href', '/g/catalog');
      }
    } finally {
      try { if (op) expect((await context.request.post(`${origin}/fixture/delete`, { headers: { origin }, form: { no: op, password }, maxRedirects: 0 })).status()).toBe(303); }
      finally { await context.close(); }
    }
  });
}
