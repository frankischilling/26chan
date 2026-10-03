import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

test('release local previews and comment readers preserve every saved Test markup choice', async () => {
  const source = JSON.parse(await readFile(new URL('../../fixtures/wordfilter-posting-reference.json', import.meta.url), 'utf8'));
  const cases = source.profiles.test.filter(case_ => /\[(?:code|spoiler|sjis)\]/.test(case_.input));
  assert.equal(cases.length, 432);
  const files = {};
  for (const name of ['native-filter.v1.js', 'native-backlinks.v1.js']) {
    files[`/static/${name}`] = await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8');
  }
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext();
    const page = await context.newPage(), unexpected = [], errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await context.route('**/*', async route => {
      const url = new URL(route.request().url());
      if (url.origin === 'https://wordfilters.example' && files[url.pathname]) {
        await route.fulfill({ contentType: 'text/javascript', body: files[url.pathname] });
      } else if (url.href === 'https://wordfilters.example/') {
        await route.fulfill({ contentType: 'text/html', body: '<!doctype html><html><body></body></html>' });
      } else { unexpected.push(url.href); await route.abort(); }
    });
    await page.goto('https://wordfilters.example/');
    const results = await page.evaluate(async cases_ => {
      const { localQuoteTree, prepareQuotePost } = await import('/static/native-filter.v1.js');
      const { createCommentProjection } = await import('/static/native-backlinks.v1.js');
      const projection = createCommentProjection();
      const context_ = { origin: location.origin, board: 'g', thread: '100', mediaOrigin: '' };
      return cases_.map(case_ => {
        document.body.innerHTML = '<article class="postContainer opContainer" id="pc100"><div class="post op" id="p100">'
          + '<div class="postInfo" id="pi100"><span class="name">Anonymous</span><span class="postNum">'
          + '<a href="/g/thread/100#p100" title="Link to this post">No.</a>'
          + '<a href="/g/thread/100?quote=100#reply" title="Reply to this post">100</a></span></div>'
          + `<blockquote class="postMessage" id="m100">${case_.final.replace(/<\/5(?:pan|p4n)?>/g, '')}</blockquote></div></article>`;
        const message = document.getElementById('m100');
        const html = projection.html(message), text = projection.text(message);
        const tree = localQuoteTree(document.getElementById('pc100'), context_, '100', projection);
        const copied = prepareQuotePost(tree, context_, '100').build(document).querySelector('.postMessage');
        return { sameHtml: copied.innerHTML === message.innerHTML, sameText: copied.textContent === text,
          bounded: html.length < 4096 && copied.querySelectorAll('[id],script,img,iframe,style').length === 0 };
      });
    }, cases);
    assert.ok(results.every(result => result.sameHtml && result.sameText && result.bounded));
    assert.deepEqual(errors, []);
    assert.deepEqual(unexpected, []);
  } finally { await browser.close(); }
});
