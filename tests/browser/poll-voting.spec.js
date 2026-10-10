import { test, expect } from '@playwright/test';
import { withOwnedVotingPoll } from './helpers/poll-fixture.js';

const origin = 'http://127.0.0.1:3000';
// The missing-selection response uses the shared error page and its styles.
const styles = new Set(['/static/polls.v1.css', '/static/board.css', '/static/theme.css', '/static/flags/flags.css', '/static/flags/board-types.css']);
const images = new Set(['/static/notifications/favicon.ico', '/static/themes/fade.png', '/static/themes/fade-blue.png']);

async function boundedPage(page, response) {
  expect(response.status()).toBe(200);
  expect(response.headers()['cache-control']).toContain('no-store');
  const csp = response.headers()['content-security-policy'].split(';').map(value => value.trim());
  for (const directive of ["script-src 'none'", "connect-src 'none'", "worker-src 'none'", "form-action 'self'"]) {
    expect(csp).toContain(directive);
  }
  await expect(page.locator('script, iframe, object, embed, svg, img, [onclick]')).toHaveCount(0);
  const overflow = await page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    width: document.documentElement.scrollWidth,
    wide: [...document.querySelectorAll('.pollPage, #entries, #entries td, #entries th')]
      .filter(node => node.getBoundingClientRect().right > document.documentElement.clientWidth + 1
        || node.getBoundingClientRect().left < -1).length,
    cookie: document.cookie, injected: window.pollInjected,
    storage: [localStorage.length, sessionStorage.length],
  }));
  expect(overflow.width).toBeLessThanOrEqual(overflow.viewport + 1);
  expect(overflow.wide).toBe(0);
  expect(overflow.cookie).toBe('');
  expect(overflow.injected).toBeUndefined();
  expect(overflow.storage).toEqual([0, 0]);
}

async function follow(page, label, url) {
  const navigation = page.waitForResponse(response => response.url() === url && response.request().isNavigationRequest());
  await page.getByRole('link', { name: label, exact: true }).click();
  await page.waitForURL(url);
  await page.waitForLoadState('load');
  const response = await navigation;
  expect(response.headers()['set-cookie']).toBeUndefined();
  await boundedPage(page, response);
  return response;
}

async function recordedOptions(page, fixture, response) {
  expect(response.headers()['cache-control']).toBe('private, no-store');
  expect(response.headers()['set-cookie']).toBeUndefined();
  await expect(page.locator('#poll-title')).toHaveText(fixture.title);
  expect(await page.locator('#poll-desc').textContent()).toBe(fixture.description);
  await expect(page.locator('.pollOptions li')).toHaveText(fixture.captions);
  await expect(page.locator('.pollPage')).toContainText('Your vote has been recorded. These options are read-only.');
  await expect(page.locator('.pollPage')).not.toContainText('Voting is unavailable.');
  await expect(page.locator('form, input, button, select, textarea, [data-tkn], .pollResults')).toHaveCount(0);
}

for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
  test(`native poll voting works with and without scripts at ${viewport.width}px`, async ({ browser }) => {
    test.setTimeout(90_000);
    await withOwnedVotingPoll(async fixture => {
      const contexts = [], pages = [], credentials = [], unexpected = [], errors = [], failed = [], posts = [];
      const catalogueUrl = `${origin}/polls`, pollUrl = `${origin}/polls/${fixture.poll}`, resultUrl = `${origin}/polls/results/${fixture.poll}`;
      try {
        for (const javascript of [true, false]) {
          const context = await browser.newContext({ viewport, javaScriptEnabled: javascript,
            ...(viewport.width === 390 ? { isMobile: true, hasTouch: true } : {}) });
          contexts.push(context);
          context.on('request', request => {
            const url = new URL(request.url());
            const document = request.resourceType() === 'document' && [catalogueUrl, pollUrl, resultUrl].includes(url.href)
              && (request.method() === 'GET' || (request.method() === 'POST' && url.href === pollUrl));
            const resource = request.method() === 'GET' && ((request.resourceType() === 'stylesheet' && styles.has(url.pathname))
              || (request.resourceType() === 'image' && images.has(url.pathname)));
            if (url.origin !== origin || url.search || (!document && !resource)) unexpected.push(`${request.method()} ${request.resourceType()} ${url.pathname}`);
            if (request.method() === 'POST') posts.push(new URLSearchParams(request.postData()));
          });
          context.on('requestfailed', request => failed.push(request.resourceType()));
          context.on('response', response => { if (response.status() >= 400) failed.push(response.status()); });
          const page = await context.newPage(); pages.push(page);
          page.on('pageerror', error => errors.push(error.message));
          const response = await page.goto(pollUrl);
          await boundedPage(page, response);
          await expect(page.locator('#poll-title')).toHaveText(fixture.title);
          expect(await page.locator('#poll-desc').textContent()).toBe(fixture.description);
          await expect(page.locator('#entries td:not(.col-opt)')).toHaveText(fixture.captions);
          await expect(page.locator('#poll-form')).toHaveAttribute('action', '');
          await expect(page.locator('#poll-form')).toHaveAttribute('method', 'POST');
          await expect(page.locator('#poll-form')).toHaveAttribute('enctype', 'application/x-www-form-urlencoded');
          const token = await page.locator('[name=_ptkn]').inputValue();
          expect(await page.locator('body').getAttribute('data-tkn')).toBe(token);
          const cookies = await context.cookies();
          credentials.push(cookies);
          expect(cookies).toHaveLength(1);
          expect(cookies[0]).toMatchObject({ name: 'board-poll', domain: '127.0.0.1', path: '/', httpOnly: true, sameSite: 'Strict', secure: false });
        }
        // The supplied source radios have no required attribute. A missing choice
        // reaches the server, which rejects it without recording a receipt.
        const missingChoice = pages[0].waitForResponse(response => response.url() === pollUrl && response.request().method() === 'POST');
        await pages[0].getByRole('button', { name: 'Vote', exact: true }).click();
        expect((await missingChoice).status()).toBe(422);
        expect(posts).toHaveLength(1);
        expect([...posts[0].keys()].sort()).toEqual(['_ptkn', 'action']);
        expect(fixture.snapshot()).toEqual({ votes: 6, newVotes: 0, open: true, receipts: 0, scores: [2, 4] });
        await boundedPage(pages[0], await pages[0].goto(pollUrl));

        for (let index = 0; index < pages.length; index++) {
          const page = pages[index], token = await page.locator('[name=_ptkn]').inputValue();
          await page.getByRole('radio', { name: fixture.captions[index], exact: true }).check();
          const submitted = page.waitForResponse(response => response.url() === pollUrl && response.request().method() === 'POST');
          const displayed = page.waitForResponse(response => response.url() === resultUrl && response.request().isNavigationRequest());
          await page.getByRole('button', { name: 'Vote', exact: true }).click();
          const accepted = await submitted;
          expect(accepted.status()).toBe(303);
          expect(accepted.headers()['set-cookie']).toBeUndefined();
          await page.waitForURL(resultUrl);
          await boundedPage(page, await displayed);
          const post = posts[index + 1];
          expect([...post.keys()].sort()).toEqual(['_ptkn', 'action', 'id']);
          expect(post.get('action')).toBe('vote'); expect(post.get('id')).toBe(index ? '17' : '41');
          expect(post.get('_ptkn')).toBe(token);
          await expect(page.locator('form, [data-tkn]')).toHaveCount(0);
          await expect(page.locator('.pollTotal')).toHaveText(`Total votes: ${7 + index}`);
          await expect(page.locator('.pollResults th')).toHaveText(index
            ? ['37.5% (3)', '62.5% (5)'] : ['42.86% (3)', '57.14% (4)']);
          const recorded = fixture.snapshot();
          expect(recorded).toEqual({ votes: 7 + index, newVotes: 1 + index,
            open: true, receipts: 1 + index, scores: [3, 4 + index] });
          const options = await follow(page, 'Back to Options', pollUrl);
          await recordedOptions(page, fixture, options);
          expect(await contexts[index].cookies()).toEqual(credentials[index]);
          expect(fixture.snapshot()).toEqual(recorded);
          await follow(page, 'Back to Polls', catalogueUrl);
          await expect(page.locator('#intro')).toContainText('artistic works of fiction and falsehood.');
          await expect(page.locator('form, input, button, [data-tkn]')).toHaveCount(0);
          expect(await contexts[index].cookies()).toEqual(credentials[index]);
          expect(fixture.snapshot()).toEqual(recorded);
          // A direct revisit also stays read-only and keeps the same receipt.
          const returned = await page.goto(pollUrl);
          await boundedPage(page, returned);
          await recordedOptions(page, fixture, returned);
          await follow(page, 'View Results', resultUrl);
          await expect(page.locator('.pollResults')).toBeVisible();
          await expect(page.locator('.pollTotal')).toHaveText(`Total votes: ${7 + index}`);
          await expect(page.locator('form, [data-tkn]')).toHaveCount(0);
          expect(await contexts[index].cookies()).toEqual(credentials[index]);
          expect(fixture.snapshot()).toEqual(recorded);
        }
        expect(fixture.snapshot()).toEqual({ votes: 8, newVotes: 2, open: true, receipts: 2, scores: [3, 5] });
        await expect(pages[1].locator('.pollResults th')).toHaveText(['37.5% (3)', '62.5% (5)']);
        fixture.close();
        const closed = await pages[1].goto(pollUrl);
        expect(closed.headers()['set-cookie']).toBeUndefined();
        await boundedPage(pages[1], closed);
        await expect(pages[1].locator('.pollPage')).toContainText('Voting is unavailable.');
        await expect(pages[1].locator('form, input, button, [data-tkn]')).toHaveCount(0);
        // Retry the exact submitted form through the same private browser cookie.
        // The API client does not add a page request to the native-form witness.
        const replay = await contexts[0].request.post(pollUrl, { headers: { Origin: origin },
          form: Object.fromEntries(posts[1]), maxRedirects: 0 });
        expect(replay.status()).toBe(303);
        expect(fixture.snapshot()).toEqual({ votes: 8, newVotes: 2, open: false, receipts: 2, scores: [3, 5] });
        expect(errors).toEqual([]); expect(failed).toEqual([422]); expect(unexpected).toEqual([]);
      } finally { for (const context of contexts) await context.close(); }
    });
  });
}
