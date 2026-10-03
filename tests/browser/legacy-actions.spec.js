import { test, expect } from '@playwright/test';

test.use({ trace: 'off' });

const origin = 'http://127.0.0.1:3000';

for (const javaScriptEnabled of [false, true]) {
  test(`legacy report form and authenticated deletion persist with JavaScript ${javaScriptEnabled ? 'enabled' : 'disabled'}`, async ({ browser }) => {
    const context = await browser.newContext({ javaScriptEnabled });
    const page = await context.newPage();
    const password = 'owned-legacy-browser-password';
    let id;
    try {
      const created = await context.request.post(`${origin}/fixture/imgboard.php`, {
        headers: { Origin: origin }, maxRedirects: 0,
        form: { mode: 'regist', sub: 'Owned legacy action', com: 'Synthetic legacy route browser check.', pwd: password },
      });
      expect(created.status()).toBe(303);
      id = /#p(\d+)$/.exec(created.headers().location)[1];

      await page.goto(`${origin}/fixture/imgboard.php?mode=report&no=${id}`);
      await expect(page.getByRole('heading', { name: `Report post No.${id}`, exact: true })).toBeVisible();
      await expect(page.getByRole('link', { name: 'Return to post' })).toHaveAttribute('href', `/fixture/thread/${id}#p${id}`);
      await page.getByLabel('Report reason', { exact: true }).fill('Owned report with <script>literal markup</script>.');
      await page.getByRole('button', { name: 'Report post', exact: true }).click();
      await expect(page.getByRole('heading', { name: 'Report received', exact: true })).toBeVisible();
      expect((await context.request.get(`${origin}/fixture/thread/${id}.json`)).status()).toBe(200);

      if (javaScriptEnabled) {
        await page.goto(`${origin}/fixture/thread/${id}`);
        // The released client sends FormData. Keep the project's explicit
        // deletion password while exercising the browser's real Origin/CSP.
        const deleted = await page.evaluate(async ({ id, password }) => {
          const body = new FormData();
          body.append('mode', 'usrdel'); body.append(id, 'delete'); body.append('pwd', password);
          const response = await fetch('/fixture/imgboard.php', { method: 'POST', body, credentials: 'same-origin' });
          return { status: response.status, body: await response.text() };
        }, { id, password });
        expect(deleted.status).toBe(200);
        expect(deleted.body).toContain('Updating index');
        expect(deleted.body).not.toContain(password);
      } else {
        const deleted = await context.request.post(`${origin}/fixture/imgboard.php`, {
          headers: { Origin: origin }, maxRedirects: 0,
          form: { mode: 'usrdel', [id]: 'delete', pwd: password },
        });
        expect(deleted.status()).toBe(200);
        expect(await deleted.text()).toContain('Updating index');
      }
      expect((await context.request.get(`${origin}/fixture/thread/${id}.json`)).status()).toBe(404);
      const gone = await page.goto(`${origin}/fixture/imgboard.php?mode=report&no=${id}`);
      expect(gone.status()).toBe(404);
      await expect(page.getByRole('button', { name: 'Report post', exact: true })).toHaveCount(0);
      id = null;
    } finally {
      try {
        if (id) {
          const deleted = await context.request.post(`${origin}/fixture/delete`, {
            headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
          });
          expect([303, 404]).toContain(deleted.status());
        }
      } finally { await context.close(); }
    }
  });
}
