import { test, expect } from '@playwright/test';
import { withOwnedPolls } from './helpers/poll-fixture.js';

const origin = 'http://127.0.0.1:3000';
const stylesheets = new Set(['/static/board.css', '/static/theme.css',
  '/static/flags/flags.css', '/static/flags/board-types.css']);

async function inertPage(page, response) {
  expect(response.status()).toBe(200);
  expect(response.headers()['set-cookie']).toBeUndefined();
  const csp = response.headers()['content-security-policy'].split(';').map(value => value.trim());
  for (const directive of ["script-src 'none'", "connect-src 'none'", "worker-src 'none'"]) {
    expect(csp).toContain(directive);
  }
  await expect(page.locator('.pollPage')).toBeVisible();
  await expect(page.locator('.pollPage')).toContainText('Voting is unavailable.');
  for (const element of await page.locator('.pollPage h1, #poll-desc, #entries li, #entries a, #entries caption, #entries th, #entries td, .pollTotal').all()) {
    await expect(element).toBeVisible();
  }
  await expect(page.locator('.pollPage p').filter({ hasText: 'Voting is unavailable.' })).toBeVisible();
  await expect(page.locator('script, form, input, button, select, textarea, iframe, object, embed, svg, img, [data-tkn], [data-cmd], [role="button"]')).toHaveCount(0);
  expect(await page.evaluate(() => ({
    injected: window.pollInjected === undefined,
    handlers: [...document.querySelectorAll('*')].flatMap(node => [...node.attributes]
      .filter(attribute => /^on/i.test(attribute.name)).map(attribute => attribute.name)),
    cookies: document.cookie,
    storage: [localStorage.length, sessionStorage.length],
  }))).toMatchObject({ injected: true, handlers: [], cookies: '', storage: [0, 0] });
  const overflow = await page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    document: document.documentElement.scrollWidth,
    body: document.body.scrollWidth,
    entries: [...document.querySelectorAll('.pollPage, #entries, #entries li, #entries td, #entries th')]
      .filter(node => node.getBoundingClientRect().right > document.documentElement.clientWidth + 1
        || node.getBoundingClientRect().left < -1).map(node => node.tagName),
  }));
  expect(overflow.document).toBeLessThanOrEqual(overflow.viewport + 1);
  expect(overflow.body).toBeLessThanOrEqual(overflow.viewport + 1);
  expect(overflow.entries).toEqual([]);
}

for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test(`published polls stay ordered, escaped and read-only at ${viewport.width}px`, async ({ browser }) => {
    test.setTimeout(60_000);
    await withOwnedPolls(async fixture => {
      const context = await browser.newContext({ viewport, javaScriptEnabled: true,
        ...(viewport.width === 390 ? { isMobile: true, hasTouch: true } : {}) });
      const unexpected = [], errors = [], failed = [];
      const pages = new Set(['/polls', `/polls/${fixture.first}`, `/polls/results/${fixture.first}`, `/polls/${fixture.lowId}`]);
      try {
        const page = await context.newPage();
        context.on('request', request => {
          const url = new URL(request.url());
          const allowed = url.origin === origin && !url.search && request.method() === 'GET'
            && ((request.resourceType() === 'document' && pages.has(url.pathname))
              || (request.resourceType() === 'stylesheet' && stylesheets.has(url.pathname))
              || (request.resourceType() === 'image' && ['/static/notifications/favicon.ico', '/static/themes/fade.png'].includes(url.pathname)));
          if (!allowed) unexpected.push(`${request.method()} ${request.resourceType()} ${request.url()}`);
        });
        context.on('requestfailed', request => failed.push(request.url()));
        context.on('response', response => {
          if (response.status() >= 400) failed.push(`${response.status()} ${response.url()}`);
        });
        page.on('pageerror', error => errors.push(error.message));
        page.on('websocket', socket => unexpected.push(`websocket ${socket.url()}`));
        context.on('serviceworker', worker => unexpected.push(`serviceworker ${worker.url()}`));

        await inertPage(page, await page.goto(`${origin}/polls`));
        const links = await page.locator('.pollCatalogue a').evaluateAll(nodes => nodes.map(node => node.getAttribute('href')));
        expect(links.indexOf(`/polls/${fixture.first}`)).toBeGreaterThanOrEqual(0);
        expect(links.indexOf(`/polls/${fixture.second}`)).toBeGreaterThan(links.indexOf(`/polls/${fixture.first}`));
        expect(links).not.toContain(`/polls/${fixture.hidden}`);
        await expect(page.locator(`.pollCatalogue a[href="/polls/${fixture.first}"]`)).toHaveText(fixture.title);

        const follow = async (link, path) => {
          const response = page.waitForResponse(response => response.url() === `${origin}${path}` && response.request().isNavigationRequest());
          await link.click();
          await page.waitForURL(`${origin}${path}`);
          await page.waitForLoadState('load');
          await inertPage(page, await response);
        };
        await follow(page.locator(`.pollCatalogue a[href="/polls/${fixture.first}"]`), `/polls/${fixture.first}`);
        await expect(page.locator('#poll-title')).toHaveText(fixture.title);
        expect(await page.locator('#poll-desc').textContent()).toBe(fixture.description);
        await expect(page.locator('.pollOptions li')).toHaveText(fixture.captions);
        await follow(page.getByRole('link', { name: 'View Results', exact: true }), `/polls/results/${fixture.first}`);
        await expect(page.locator('#poll-title')).toHaveText(fixture.title);
        expect(await page.locator('#poll-desc').textContent()).toBe(fixture.description);
        await expect(page.locator('.pollResults tbody th')).toHaveText(fixture.captions);
        await expect(page.locator('.pollResults tbody td')).toHaveText(['33.33% (2)', '50% (3)', '0% (0)']);
        await expect(page.locator('.pollTotal')).toHaveText('Total votes: 6');
        await follow(page.getByRole('link', { name: 'Back to Options', exact: true }), `/polls/${fixture.first}`);
        await follow(page.getByRole('link', { name: 'Back to Polls', exact: true }), '/polls');
        // A one-digit path must not be classified as a board thread page.
        await inertPage(page, await page.goto(`${origin}/polls/${fixture.lowId}`));
        await page.waitForLoadState('networkidle');
        expect(await context.cookies()).toEqual([]);
        expect(errors).toEqual([]);
        expect(failed).toEqual([]);
        expect(unexpected).toEqual([]);
      } finally { await context.close(); }
    });
  });
}
