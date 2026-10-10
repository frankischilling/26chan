import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

test('release local previews and comment readers preserve every saved Test markup choice', async () => {
  const source = JSON.parse(await readFile(new URL('../../fixtures/wordfilter-posting-reference.json', import.meta.url), 'utf8'));
  const cases = source.profiles.test.filter(case_ => /\[(?:code|spoiler|sjis)\]/.test(case_.input));
  assert.equal(cases.length, 432);
  const drawing = JSON.parse(await readFile(new URL('../../crates/domain/tests/fixtures/drawing-annotation/cases.json', import.meta.url), 'utf8'));
  const annotations = drawing.time.filter(case_ => case_.expected_text.startsWith('<br><br><small>')
    && !case_.expected_text.includes('Replay:')).map(case_ => ({
    input: case_.id, final: `Saved drawing${case_.expected_text}`, annotation: true,
    annotationText: case_.expected_text.replace(/<[^>]+>/g, '').replaceAll('&gt;', '>'),
  }));
  assert.ok(annotations.some(case_ => case_.input === 'source'));
  assert.ok(annotations.length > 10);
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
        const small = copied.querySelector('small');
        const preservesAnnotation = !case_.annotation || (
          copied.querySelectorAll('small').length === 1
          && small.firstElementChild?.tagName === 'B'
          && small.firstElementChild.textContent === 'Oekaki Post'
          && small.textContent === case_.annotationText
          && small.outerHTML === message.querySelector('small')?.outerHTML
          && html.includes('<small><b>Oekaki Post</b>')
          && text === `Saved drawing${case_.annotationText}`);
        return { sameHtml: copied.innerHTML === message.innerHTML, sameText: copied.textContent === text,
          bounded: html.length < 4096 && copied.querySelectorAll('[id],script,img,iframe,style').length === 0,
          preservesAnnotation };
      });
    }, [...cases, ...annotations]);
    for (const [index, result] of results.entries()) {
      const label = index < cases.length ? cases[index].input : annotations[index - cases.length].input;
      assert.ok(result.sameHtml && result.sameText && result.bounded && result.preservesAnnotation,
        `${label}: ${JSON.stringify(result)}`);
    }
    assert.deepEqual(errors, []);
    assert.deepEqual(unexpected, []);
  } finally { await browser.close(); }
});

test('release local previews preserve drawing annotations and omit the drawing Edit control', async () => {
  const source = JSON.parse(await readFile(new URL('../../crates/domain/tests/fixtures/drawing-annotation/cases.json', import.meta.url), 'utf8'));
  const annotation = source.time.find(case_ => case_.id === 'source').expected_text;
  const files = {};
  for (const name of ['native-filter.v1.js', 'native-backlinks.v1.js']) {
    files[`/static/${name}`] = await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8');
  }
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext(), page = await context.newPage();
    const unexpected = [], errors = [];
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
    const result = await page.evaluate(async annotation_ => {
      const { localQuoteTree, prepareQuotePost } = await import('/static/native-filter.v1.js');
      const { createCommentProjection } = await import('/static/native-backlinks.v1.js');
      const mediaOrigin = 'https://drawing-media.example', image = `${mediaOrigin}/i/123.png`;
      // Keep the fixture in an inert template: this reader test needs the exact
      // post DOM, while image fetch and pixel integrity have separate coverage.
      const holder = document.createElement('template');
      holder.innerHTML = '<article class="postContainer replyContainer" id="pc101"><div class="post reply" id="p101">'
        + '<div class="postInfo" id="pi101"><span class="name">Anonymous</span><span class="postNum">'
        + '<a href="/i/thread/100#p101" title="Link to this post">No.</a>'
        + '<a href="/i/thread/100?quote=101#reply" title="Reply to this post">101</a></span></div>'
        + `<div class="file" id="f101"><div class="fileText" id="fT101">File: <a href="${image}" target="_blank" rel="noopener noreferrer">tegaki.png</a> (4 KB, 400x400)</div>`
        + `<a class="fileThumb" href="${image}" target="_blank" rel="noopener noreferrer"><img src="${mediaOrigin}/i/123s.jpg" alt="tegaki.png" width="200" height="200"><div class="mFileInfo mobile">4 KB PNG</div></a></div>`
        + `<blockquote class="postMessage" id="m101">Saved drawing${annotation_}</blockquote></div></article>`;
      const article = holder.content.firstElementChild;
      // This is the transient wrapper emitted by mountDrawingEditLinks.
      const wrapper = document.createElement('small'), link = document.createElement('a');
      wrapper.dataset.drawingEditWrap = '';
      link.href = '#'; link.dataset.drawingEdit = '';
      link.title = 'Open in Tegaki'; link.setAttribute('aria-label', 'Edit image from post 101 in Tegaki');
      link.textContent = 'Edit'; wrapper.append(' ', link); article.querySelector('.fileText').append(wrapper);
      const before = article.outerHTML, projection = createCommentProjection();
      const context_ = { origin: location.origin, board: 'i', thread: '100', mediaOrigin };
      const tree = localQuoteTree(article, context_, '101', projection);
      const copied = prepareQuotePost(tree, context_, '101').build(document);
      const copiedImage = copied.querySelector('.fileThumb img');
      return {
        comment: copied.querySelector('.postMessage').innerHTML,
        originalComment: article.querySelector('.postMessage').innerHTML,
        fileText: copied.querySelector('.fileText').textContent,
        image: [copied.querySelector('.fileThumb').getAttribute('href'), copiedImage.getAttribute('src'),
          copiedImage.getAttribute('alt'), copiedImage.getAttribute('width'), copiedImage.getAttribute('height')],
        smallCount: copied.querySelectorAll('small').length,
        controls: copied.querySelectorAll('[data-drawing-edit-wrap], [data-drawing-edit], .fileText small').length,
        unchanged: article.outerHTML === before,
        sourceControl: article.querySelector('[data-drawing-edit]')?.textContent,
      };
    }, annotation);
    assert.equal(result.comment, `Saved drawing${annotation}`);
    assert.equal(result.originalComment, result.comment);
    assert.equal(result.fileText, 'File: tegaki.png (4 KB, 400x400)');
    assert.deepEqual(result.image, ['https://drawing-media.example/i/123.png',
      'https://drawing-media.example/i/123s.jpg', 'tegaki.png', '200', '200']);
    assert.equal(result.smallCount, 1);
    assert.equal(result.controls, 0);
    assert.equal(result.unchanged, true);
    assert.equal(result.sourceControl, 'Edit');
    assert.deepEqual(errors, []);
    assert.deepEqual(unexpected, []);
  } finally { await browser.close(); }
});
