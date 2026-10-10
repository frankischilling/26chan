import { test, expect, attachComparedImage } from '../helpers/visual-diagnostics.js';

const cases = ['catalogue', 'empty-catalogue', 'options', 'options-no-description', 'results', 'empty-results'];

// The original templates have no viewport meta element. Preserve that behavior
// in both the desktop viewport and a real mobile layout viewport.
for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test.describe(`source poll pages at ${viewport.width}px`, () => {
    test.use({ viewport, javaScriptEnabled: false,
      ...(viewport.width === 390 ? { isMobile: true, hasTouch: true } : {}) });
    for (const name of cases) {
      test(`${name} matches the supplied PHP template and referenced CSS`, async ({ context }, info) => {
        const failures = [], external = [];
        context.on('requestfailed', request => failures.push(request.resourceType()));
        context.on('response', response => { if (response.status() >= 400) failures.push(response.status()); });
        context.on('request', request => { if (new URL(request.url()).origin !== 'http://127.0.0.1:3000') external.push(request.resourceType()); });
        const reference = await context.newPage(), production = await context.newPage();
        const responses = await Promise.all([
          reference.goto(`/poll-visual/reference/${name}`),
          production.goto(`/poll-visual/production/${name}`),
        ]);
        for (const response of responses) expect(response.status()).toBe(200);
        for (const page of [reference, production]) {
          await expect(page.locator('#title')).toHaveText('4chan Polls');
          await expect(page).toHaveTitle('Polls - 4chan');
          await expect(page.locator('meta[name=viewport], script')).toHaveCount(0);
          await expect(page.locator('body')).toHaveCSS('background-color', 'rgb(255, 255, 238)');
          await expect(page.locator('body')).toHaveCSS('color', 'rgb(128, 0, 0)');
          await page.evaluate(async () => {
            await document.fonts.ready;
            const image = new Image();
            image.src = '/static/themes/fade.png';
            await image.decode();
          });
        }
        const contract = page => page.evaluate(() => ({
          links: [...document.querySelectorAll('a')].map(link => [link.textContent,
            link.getAttribute('href').replace(/^polls\//, '/polls/')]),
          form: [...document.querySelectorAll('form')].map(form => [form.getAttribute('action'),
            form.getAttribute('method'), form.getAttribute('enctype')]),
          fields: [...document.querySelectorAll('input, button')].map(input => [input.type,
            input.name, input.value, input.required || false]),
        }));
        expect(await contract(production)).toEqual(await contract(reference));
        if (name.startsWith('options')) {
          // Source captions are ordinary table cells; clicking one must not
          // introduce label-click selection that the source did not have.
          for (const page of [reference, production]) {
            await page.locator('#entries td:not(.col-opt)').first().click();
            await expect(page.locator('input:checked')).toHaveCount(0);
          }
        }
        const expected = await reference.screenshot({ fullPage: true, animations: 'disabled' });
        const actual = await production.screenshot({ fullPage: true, animations: 'disabled' });
        if (!actual.equals(expected)) {
          await attachComparedImage(info, `poll-${name}-source-${viewport.width}`, expected);
          await attachComparedImage(info, `poll-${name}-production-${viewport.width}`, actual);
        }
        expect(actual.equals(expected), 'Complete page pixels must match the independently rendered source.').toBe(true);
        expect(failures).toEqual([]);
        expect(external).toEqual([]);
      });
    }
  });
}
