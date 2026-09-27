import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const fields = JSON.parse(await readFile(new URL('../fixtures/catalog-search-fields.json', import.meta.url), 'utf8'));
const grammar = JSON.parse(await readFile(new URL('../fixtures/catalog-search-cases.json', import.meta.url), 'utf8'));

test('native field composition and queries match the shared serialized contract', async ({ page }) => {
  const actual = await page.evaluate(({ fields, grammar }) => {
    const escape = new RegExp('(' + grammar.escape_characters.map(character => '\\' + character).join('|') + ')', 'g');
    return fields.composition_cases.map(entry => {
      const text = entry.subject ? `<b>${entry.subject}</b>${entry.teaser ? `: ${entry.teaser}` : ''}` : entry.teaser;
      return { text, matches: entry.checks.map(check => {
        const pattern = new RegExp(check.query.replace(escape, '\\$1'), grammar.flags);
        return pattern.test(text) || (entry.file !== null && pattern.test(entry.file));
      }) };
    });
  }, { fields, grammar });
  expect(actual).toEqual(fields.composition_cases.map(entry => ({ text: entry.text, matches: entry.checks.map(check => check.matches) })));
});

test('release catalog search consumes the shared serialized field contract', async ({ page }) => {
  const origin = 'http://127.0.0.1:3000';
  const response = await page.goto(origin + '/test/catalog?q=');
  expect(response).not.toBeNull();
  const csp = response.headers()['content-security-policy'];
  expect(csp).toBeTruthy();
  const attribute = value => value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
  const cards = fields.composition_cases.map((entry, index) => {
    const id = index + 1;
    return '<section class="thread" id="thread-' + id + '" data-thread-id="' + id + '" data-bumped="100" data-latest-reply="" data-replies="0" data-sticky="false"><a class="catalogThumb" href="/test/thread/' + id + '" data-search-text="' + attribute(entry.text) + '" data-search-file="' + attribute(entry.file ?? '') + '" data-has-file="' + (entry.file !== null) + '"></a><div class="teaser">fixture ' + id + '</div></section>';
  }).join('');
  const html = '<!doctype html><form id="ctrl" action="/test/catalog" method="get"><select id="order-ctrl" name="order"><option value="alt">Bump</option></select><select id="size-ctrl" name="size"><option value="small">Small</option></select><select id="teaser-ctrl" name="teaser"><option value="on">On</option></select><input id="qf-box" name="q" type="search"><button type="submit">Apply</button><a id="catalog-reset" href="/test/catalog">Reset</a></form><div id="threads" class="catalog extended-small">' + cards + '</div><template id="catalogFiltered"></template><script src="/static/catalog-preferences.v1.js" defer></script>';
  await page.route('**/test/catalog?field-contract=1', route => route.fulfill({
    headers: { 'content-security-policy': csp, 'content-type': 'text/html; charset=utf-8' }, body: html,
  }));
  await page.goto(origin + '/test/catalog?field-contract=1');
  for (const [index, entry] of fields.composition_cases.entries()) {
    const link = page.locator('#thread-' + (index + 1) + ' .catalogThumb');
    await expect(link).toHaveAttribute('data-search-text', entry.text);
    await expect(link).toHaveAttribute('data-search-file', entry.file ?? '');
    await expect(link).toHaveAttribute('data-has-file', String(entry.file !== null));
  }
  let navigations = 0;
  page.on('request', request => { if (request.isNavigationRequest() && request.frame() === page.mainFrame()) navigations += 1; });
  for (const [index, entry] of fields.composition_cases.entries()) {
    for (const check of entry.checks) {
      await page.locator('#qf-box').fill(check.query);
      await page.getByRole('button', { name: 'Apply', exact: true }).click();
      await expect(page.locator('#thread-' + (index + 1))).toHaveCount(check.matches ? 1 : 0);
    }
  }
  expect(navigations).toBe(0);
});

test('server GET and release live search agree on escaped formatted fields from persisted posts', async ({ browser }) => {
  const origin = 'http://127.0.0.1:3000';
  const marker = `Fields${Date.now()}`;
  const password = 'catalog-field-fixture-password';
  const cases = [
    { subject: `${marker} A&B`, comment: 'line one\n\nline   two', text: `<b>${marker} A&amp;B</b>: line one line   two` },
    { subject: `${marker} "quoted"`, comment: '[spoiler]quiet[/spoiler]\n>>1', text: `<b>${marker} &quot;quoted&quot;</b>: <s>quiet</s> &gt;&gt;1` },
    { subject: `${marker} <tag>`, comment: 'literal <b>not HTML</b>', text: `<b>${marker} &lt;tag&gt;</b>: literal &lt;b&gt;not HTML&lt;/b&gt;` },
  ];
  const noScript = await browser.newContext({ javaScriptEnabled: false });
  const liveContext = await browser.newContext();
  const server = await noScript.newPage();
  const live = await liveContext.newPage();
  const created = [];
  try {
    for (const entry of cases) {
      await server.goto(`${origin}/test/`);
      await server.locator('#sub').fill(entry.subject);
      await server.locator('#com').fill(entry.comment);
      await server.locator('#password').fill(password);
      const submitted = server.waitForResponse(response => response.url().endsWith('/test/imgboard.php') && response.request().method() === 'POST');
      await server.getByRole('button', { name: 'Post', exact: true }).click();
      const response = await submitted;
      expect(response.status(), `Native posting status; retry-after=${response.headers()['retry-after'] ?? 'absent'}`).toBe(303);
      await expect(server).toHaveURL(/\/thread\/\d+#p\d+$/);
      entry.id = /#p(\d+)$/.exec(server.url())[1];
      created.push(entry.id);
    }
    await live.goto(`${origin}/test/catalog?q=${marker}`);
    for (const entry of cases) {
      await expect(live.locator(`#thread-${entry.id} .catalogThumb`)).toHaveAttribute('data-search-text', entry.text);
    }
    let navigations = 0;
    live.on('request', request => { if (request.isNavigationRequest()) navigations += 1; });
    for (const entry of cases) {
      const query = `^${entry.text}$`;
      expect(Array.from(query).length).toBeLessThanOrEqual(128);
      await server.goto(`${origin}/test/catalog?q=${encodeURIComponent(query)}`);
      await expect(server.locator('#threads > .thread')).toHaveCount(1);
      await expect(server.locator('#threads > .thread')).toHaveAttribute('id', `thread-${entry.id}`);
      await expect(server.locator(`#thread-${entry.id} .catalogThumb`)).toHaveAttribute('data-search-text', entry.text);
      await live.locator('#qf-box').fill(query);
      await live.getByRole('button', { name: 'Apply', exact: true }).click();
      await expect(live.locator('#threads > .thread')).toHaveCount(1);
      await expect(live.locator('#threads > .thread')).toHaveAttribute('id', `thread-${entry.id}`);
      await expect(live.locator(`#thread-${entry.id} .catalogThumb`)).toHaveAttribute('data-search-text', entry.text);
      await expect(live.locator('#threads script')).toHaveCount(0);
    }
    for (const query of [`^${marker}`, `${marker} A&B`]) {
      await server.goto(`${origin}/test/catalog?q=${encodeURIComponent(query)}`);
      await expect(server.locator('#threads > .thread')).toHaveCount(0);
      await live.locator('#qf-box').fill(query);
      await live.getByRole('button', { name: 'Apply', exact: true }).click();
      await expect(live.locator('#threads > .thread')).toHaveCount(0);
    }
    await live.locator('#qf-box').fill(marker);
    await live.getByRole('button', { name: 'Apply', exact: true }).click();
    await expect(live.locator('#threads > .thread')).toHaveCount(3);
    expect(navigations).toBe(0);
  } finally {
    for (const id of created) {
      await server.goto(`${origin}/test/thread/${id}`);
      const actions = server.locator(`#p${id} .postActions`);
      await actions.getByText('Delete or report', { exact: true }).click();
      await actions.getByLabel('Deletion password', { exact: true }).fill(password);
      await actions.getByRole('button', { name: 'Delete post', exact: true }).click();
    }
    await liveContext.close();
    await noScript.close();
  }
});
