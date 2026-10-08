import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { parseBoardPageSnapshot } from '../../apps/public/client/native-updater-snapshot.js';
import { DEPAGER_LIMITS, validateDepagerSnapshot } from '../../apps/public/client/native-depager.js';
import { NativeBoardPageTransport } from '../../apps/public/client/native-depager-transport.js';

const origin = 'https://depager.example';
const mediaOrigin = 'https://media.example';
const board = 'demo';

function post(no, thread, image = false) {
  const kind = no === thread ? 'op' : 'reply';
  const file = image
    ? `<div class="file" id="f${no}"><img src="${mediaOrigin}/${board}/${no}.png" alt="Owned" loading="lazy"></div>`
    : '';
  return `<article class="postContainer ${kind}Container" id="pc${no}"><div class="post ${kind}" id="p${no}">`
    + `<div class="postInfo" id="pi${no}"><span class="name">Anonymous</span>`
    + `<span class="postNum"><a href="/${board}/thread/${thread}#p${no}" title="Link to this post">No.</a><a href="/${board}/thread/${thread}?quote=${no}#reply" title="Reply to this post">${no}</a></span></div>${file}`
    + `<blockquote class="postMessage" id="m${no}">Owned page ${no} &lt;script&gt;literal&lt;/script&gt;</blockquote>`
    + '</div></article>';
}

function wireThread(thread, { image = false, replies = 3 } = {}) {
  const ids = Array.from({ length: Math.min(4, replies + 1) }, (_, index) => String(BigInt(thread) + BigInt(index)));
  return {
    thread, closed: false, sticky: false, archived: false, replies, images: image ? 1 : 0,
    omitted: replies - (ids.length - 1),
    posts: ids.map((no, index) => ({ no, file_deleted: false, html: post(no, thread, image && index === 1) })),
  };
}

function wirePage(page = 1, nextPage = null, threads = [wireThread('100')]) {
  return { version: 1, board, page, next_page: nextPage, threads };
}

function parsedPage(page = 1, nextPage = null, threads = [wireThread('100')], media = '') {
  const result = parseBoardPageSnapshot(JSON.stringify(wirePage(page, nextPage, threads)),
    { origin, board, page, mediaOrigin: media });
  assert.equal(result.status, 'ok');
  return result.snapshot;
}

test('parsed page contract is exact, bounded and uses canonical page progression', () => {
  const snapshot = parsedPage(1, 2, [wireThread('100'), wireThread('200')]);
  const result = validateDepagerSnapshot(snapshot, { origin, board, page: 1, mediaOrigin: '' });
  assert.equal(result.context.page, 1);
  assert.equal(result.cost.posts, 8);
  assert.ok(result.cost.nodes > 20 && result.cost.bytes > 100);

  for (const mutate of [
    value => { value.board = 'other'; },
    value => { value.page = 2; },
    value => { value.next_page = 3; },
    value => { value.threads[1].thread = value.threads[0].thread; },
    value => { value.threads[0].posts[1].no = value.threads[0].posts[0].no; },
    value => { value.threads[0].archived = true; },
    value => { value.threads[0].omitted++; },
  ]) {
    const changed = structuredClone(snapshot); mutate(changed);
    assert.throws(() => validateDepagerSnapshot(changed, { origin, board, page: 1, mediaOrigin: '' }));
  }
});

test('Depager aggregate limits keep the public page family finite', () => {
  assert.deepEqual(DEPAGER_LIMITS, {
    pages: 20, threads: 200, posts: 1000, nodes: 200000, bytes: 16777216,
    requestMs: 10000, applyMs: 10000, threshold: 350,
  });
});

test('board-page worker parser rejects hostile wire before any live DOM exists', () => {
  const deletionForm = value => `<details class="postActions"><summary>Delete or report</summary>`
    + `<form method="post" action="/${board}/delete"><input type="hidden" name="no" value="100">`
    + `<label for="delete100">Deletion password</label><input type="password" name="password" id="delete100" autocomplete="off" required${value ? ` value="${value}"` : ''}>`
    + '<button>Delete post</button></form></details>';
  const healthyForm = wirePage(1, null);
  healthyForm.threads[0].posts[0].html = healthyForm.threads[0].posts[0].html.replace(
    '</blockquote></div></article>', `</blockquote>${deletionForm('')}</div></article>`);
  assert.equal(parseBoardPageSnapshot(JSON.stringify(healthyForm), { origin, board, page: 1, mediaOrigin: '' }).status, 'ok');

  const cases = [
    raw => { raw.extra = true; },
    raw => { raw.next_page = 3; },
    raw => { raw.threads[0].archived = true; },
    raw => { raw.threads[0].posts[1].no = raw.threads[0].posts[0].no; },
    raw => { raw.threads[0].posts[0].html = raw.threads[0].posts[0].html.replace('</blockquote>', '<script>bad()</script></blockquote>'); },
    raw => { raw.threads[0].posts[0].html = raw.threads[0].posts[0].html.replace(
      '</blockquote></div></article>', `</blockquote>${deletionForm('secret')}</div></article>`); },
  ];
  for (const mutate of cases) {
    const raw = wirePage(1, null); mutate(raw);
    assert.equal(parseBoardPageSnapshot(JSON.stringify(raw), { origin, board, page: 1, mediaOrigin: '' }).status, 'invalid-snapshot');
  }
});

function parserWorker() {
  return {
    onmessage: null, onerror: null,
    postMessage(job) { queueMicrotask(() => this.onmessage?.({ data: parseBoardPageSnapshot(job.raw, job.context) })); },
    terminate() {},
  };
}

function response(url, body, headers = { 'content-type': 'application/json' }) {
  return { status: 200, redirected: false, url, headers: new Headers(headers), body };
}

test('board-page transport accepts bounded empty chunks and counts an empty-chunk flood', async () => {
  const url = `${origin}/_watch/${board}/page/1`;
  const bytes = new TextEncoder().encode(JSON.stringify(wirePage(1, null)));
  const normalBody = new ReadableStream({
    start(controller) {
      controller.enqueue(new Uint8Array());
      controller.enqueue(bytes.slice(0, 7));
      controller.enqueue(new Uint8Array());
      controller.enqueue(bytes.slice(7));
      controller.close();
    },
  });
  const transport = new NativeBoardPageTransport({ origin, board,
    fetcher: async () => response(url, normalBody), createWorker: parserWorker });
  assert.equal((await transport.refresh({ page: 1 })).status, 'ok');

  const emptyBody = new ReadableStream({ start(controller) { controller.close(); } });
  const empty = new NativeBoardPageTransport({ origin, board,
    fetcher: async () => response(url, emptyBody), createWorker: parserWorker });
  assert.deepEqual(await empty.refresh({ page: 1 }), { status: 'invalid-snapshot' });
  assert.equal(empty.active, null);

  let reads = 0, workerCreated = false;
  const floodBody = new ReadableStream({
    pull(controller) {
      if (reads++ <= 65536) controller.enqueue(new Uint8Array());
      else controller.close();
    },
  });
  const flood = new NativeBoardPageTransport({ origin, board,
    fetcher: async () => response(url, floodBody), createWorker: () => { workerCreated = true; return parserWorker(); } });
  assert.deepEqual(await flood.refresh({ page: 1 }), { status: 'response-limit' });
  assert.equal(workerCreated, false);
});

test('board-page transport cancellation settles one flight and cancels its locked body', async () => {
  const url = `${origin}/_watch/${board}/page/1`;
  let entered;
  const reading = new Promise(resolve => { entered = resolve; });
  let bodyCancelled = false;
  const body = new ReadableStream({
    pull() { entered(); return new Promise(() => {}); },
    cancel() { bodyCancelled = true; },
  });
  const transport = new NativeBoardPageTransport({ origin, board,
    fetcher: async () => response(url, body), createWorker: () => { throw new Error('worker must not start'); } });
  const pending = transport.refresh({ page: 1 });
  await reading;
  assert.deepEqual(await transport.refresh({ page: 1 }), { status: 'busy' });
  transport.cancel();
  assert.deepEqual(await pending, { status: 'cancelled' });
  assert.equal(bodyCancelled, true);
  assert.equal(transport.active, null);
});

test('isolated Depager DOM and fixed worker transport behavior', async t => {
  const assets = {};
  for (const [path, file] of [
    ['/client/native-depager.js', '../../apps/public/client/native-depager.js'],
    ['/client/native-depager-transport.js', '../../apps/public/client/native-depager-transport.js'],
    ['/client/native-post-tree.js', '../../apps/public/client/native-post-tree.js'],
    ['/client/native-spoiler-assets.js', '../../apps/public/client/native-spoiler-assets.js'],
    ['/client/native-spoilers.js', '../../apps/public/client/native-spoilers.js'],
    ['/static/native-filter.v1.js', '../../apps/public/static/native-filter.v1.js'],
    ['/static/thread-watcher-core.v1.js', '../../apps/public/static/thread-watcher-core.v1.js'],
  ]) assets[path] = await readFile(new URL(file, import.meta.url), 'utf8');
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup({ snapshot = parsedPage(), raw = wirePage(), config = {}, nextPage = 1,
      limits = {}, apply = 'ok', real = false, media = '' } = {}) {
      const context = await browser.newContext();
      const page = await context.newPage();
      const requests = [], errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await context.route('**/*', async route => {
        const url = new URL(route.request().url());
        if (url.origin === origin && assets[url.pathname]) {
          await route.fulfill({ contentType: 'text/javascript', body: assets[url.pathname] });
        } else if (url.href === `${origin}/${board}/`) {
          await route.fulfill({ contentType: 'text/html', body: `<!doctype html><html><body>
            <main class="board"><section class="thread" id="t10"><article class="postContainer opContainer" id="pc10">
            <div class="post op" id="p10"><div class="postInfo" id="pi10"></div><blockquote class="postMessage" id="m10">Original</blockquote>
            <input id="owned-draft" value=""></div></article></section></main>
            <nav class="pages"><a rel="next" href="/${board}/1">Next</a></nav></body></html>` });
        } else if (real && url.href === `${origin}/_watch/${board}/page/1`) {
          requests.push({ kind: 'snapshot', headers: await route.request().allHeaders() });
          await route.fulfill({ contentType: 'application/json', body: JSON.stringify(raw) });
        } else if (url.origin === mediaOrigin) {
          requests.push({ kind: 'media', url: url.href });
          await route.abort();
        } else if (url.pathname === '/favicon.ico') await route.fulfill({ status: 204 });
        else { requests.push({ kind: 'unexpected', url: url.href }); await route.abort(); }
      });
      await page.goto(`${origin}/${board}/`);
      await page.locator('#owned-draft').fill('preserve-me');
      await page.evaluate(async ({ snapshot, config, nextPage, limits, apply, real, media }) => {
        const depagerModule = await import('/client/native-depager.js');
        const transportModule = real ? await import('/client/native-depager-transport.js') : null;
        window.config = config; window.loads = []; window.cancels = 0; window.states = []; window.applies = 0;
        window.appliedResolve = null;
        window.originalThread = document.getElementById('t10');
        const createTransport = real
          ? value => new transportModule.NativeBoardPageTransport(value)
          : () => ({
            refresh({ page, signal }) { return new Promise(resolve => loads.push({ page, signal, resolve })); },
            cancel() { cancels++; },
          });
        window.parsingEvents = [];
        for (const name of ['4chanParsingDone', '4chanPageDepaged']) document.addEventListener(name, event => parsingEvents.push({ name, detail: event.detail, constructor: event.constructor.name }));
        window.depager = depagerModule.mountNativeDepager({ root: document.querySelector('.board'), board: 'demo', page: 0,
          nextPage, mediaOrigin: media, settings: () => config, limits, createTransport,
          stateChanged: value => states.push({ ...value }),
          applied: () => {
            applies++;
            if (apply === 'throw') throw new Error('owned apply failure');
            if (apply === 'hang') return new Promise(() => {});
            if (apply === 'defer') return new Promise(resolve => { appliedResolve = resolve; });
            if (apply === 'detach') document.querySelector('.board').remove();
            if (apply === 'detach-reject') {
              document.querySelector('.board').remove();
              return Promise.reject(new Error('detached apply failed'));
            }
            if (apply === 'remove-candidate') document.getElementById('t100')?.remove();
          },
        });
        window.begin = () => { window.pending = depager.loadMore(); };
        window.finishPending = () => pending;
        window.deliver = (value = snapshot, index = 0) => loads[index].resolve({ status: 'ok', snapshot: value });
        window.deliverResult = (value, index = 0) => loads[index].resolve(value);
        window.resolveApplied = () => appliedResolve?.();
      }, { snapshot, config, nextPage, limits, apply, real, media });
      return { context, page, requests, errors };
    }

    await t.test('real transport crosses the fixed worker, omits credentials and preserves original nodes and drafts', async () => {
      const raw = wirePage(1, null, [wireThread('100')]);
      const { context, page, requests, errors } = await setup({ raw, real: true });
      try {
        await context.addCookies([{ name: 'private-session', value: 'synthetic-only', url: origin }]);
        const result = await page.evaluate(() => depager.loadMore());
        assert.deepEqual(result, { status: 'ok', page: 1, added: 1 });
        assert.deepEqual(await page.evaluate(() => parsingEvents), [
          { name: '4chanParsingDone', detail: { threadId: 100, offset: 0, limit: 4 }, constructor: 'Event' },
          { name: '4chanPageDepaged', detail: { page: 1, added: 1 }, constructor: 'CustomEvent' },
        ]);
        assert.equal(await page.locator('#t100').count(), 1);
        assert.equal(await page.locator('.depageNumber').textContent(), 'Page 2');
        assert.equal(await page.locator('#m101').textContent(), 'Owned page 101 <script>literal</script>');
        assert.equal(await page.locator('.postMessage script').count(), 0);
        assert.equal(await page.locator('#owned-draft').inputValue(), 'preserve-me');
        assert.equal(await page.evaluate(() => document.getElementById('t10') === originalThread), true);
        assert.equal(await page.getByRole('link', { name: 'Next' }).getAttribute('href'), '/demo/1');
        const snapshotRequest = requests.find(request => request.kind === 'snapshot');
        assert.ok(snapshotRequest); assert.equal(snapshotRequest.headers.cookie, undefined);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('existing threads are skipped while a new thread commits atomically', async () => {
      const snapshot = parsedPage(1, null, [wireThread('10', { replies: 0 }), wireThread('100')]);
      const { context, page } = await setup({ snapshot });
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => deliver()); await page.evaluate(() => finishPending());
        assert.equal(await page.locator('#t10').count(), 1);
        assert.equal(await page.locator('#t100').count(), 1);
        assert.equal(await page.evaluate(() => document.getElementById('t10') === originalThread), true);
        assert.equal(await page.locator('#owned-draft').inputValue(), 'preserve-me');
      } finally { await context.close(); }
    });

    await t.test('one-flight admission and explicit cancellation reject late work', async () => {
      const { context, page } = await setup();
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        assert.deepEqual(await page.evaluate(() => depager.loadMore()), { status: 'busy' });
        await page.evaluate(() => depager.cancel());
        assert.equal(await page.evaluate(() => loads[0].signal.aborted), true);
        await page.evaluate(() => deliver());
        assert.deepEqual(await page.evaluate(() => finishPending()), { status: 'cancelled' });
        assert.equal(await page.locator('#t100,.depageNumber').count(), 0);
      } finally { await context.close(); }
    });

    await t.test('HTTP errors cannot synthesize completion and retry the same authoritative page', async () => {
      const { context, page } = await setup();
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => deliverResult({ status: 'http-error', httpStatus: 404 }));
        assert.deepEqual(await page.evaluate(() => finishPending()), { status: 'http-error', httpStatus: 404 });
        assert.deepEqual(await page.evaluate(() => depager.stats()), {
          pages: 0, threads: 0, posts: 0, nodes: 0, bytes: 0,
          state: 'error', nextPage: 1, busy: false, auto: false,
        });

        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 2);
        assert.equal(await page.evaluate(() => loads[1].page), 1);
        await page.evaluate(() => deliver(undefined, 1));
        assert.deepEqual(await page.evaluate(() => finishPending()), { status: 'ok', page: 1, added: 1 });
      } finally { await context.close(); }
    });

    await t.test('duplicate live post IDs reject the whole page before an approved image can start loading', async () => {
      const snapshot = parsedPage(1, null, [wireThread('100', { image: true })], mediaOrigin);
      const { context, page, requests } = await setup({ snapshot, media: mediaOrigin });
      try {
        await page.evaluate(() => { const collision = document.createElement('div'); collision.id = 'pc101'; document.body.append(collision); });
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => deliver());
        assert.equal((await page.evaluate(() => finishPending())).status, 'invalid-snapshot');
        assert.equal(await page.locator('#t100').count(), 0);
        assert.equal(requests.filter(request => request.kind === 'media').length, 0);
      } finally { await context.close(); }
    });

    await t.test('failed feature application and page suspension roll back the complete candidate', async () => {
      for (const mode of ['throw', 'hang']) {
        const { context, page } = await setup({ apply: mode, limits: mode === 'hang' ? { applyMs: 25 } : {} });
        try {
          await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
          await page.evaluate(() => deliver()); await page.evaluate(() => finishPending());
          assert.equal(await page.locator('#t100,.depageNumber').count(), 0);
          assert.equal(await page.locator('#owned-draft').inputValue(), 'preserve-me');
        } finally { await context.close(); }
      }

      const { context, page } = await setup();
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })));
        assert.equal(await page.evaluate(() => loads[0].signal.aborted), true);
        await page.evaluate(() => deliver()); await page.evaluate(() => finishPending());
        assert.equal(await page.locator('#t100,.depageNumber').count(), 0);
        await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 2);
        await page.evaluate(() => deliver(undefined, 1)); await page.evaluate(() => finishPending());
        assert.equal(await page.locator('#t100').count(), 1);
      } finally { await context.close(); }
    });

    await t.test('cancellation during feature application ignores a late completion and permits retry', async () => {
      const { context, page } = await setup({ apply: 'defer' });
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => deliver());
        await page.waitForFunction(() => depager.stats().state === 'applying' && document.getElementById('t100'));
        await page.evaluate(() => depager.cancel());
        assert.equal(await page.locator('#t100,.depageNumber').count(), 0);
        await page.evaluate(() => resolveApplied());
        assert.deepEqual(await page.evaluate(() => finishPending()), { status: 'cancelled' });
        assert.equal((await page.evaluate(() => depager.stats())).pages, 0);

        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 2);
        await page.evaluate(() => deliver(undefined, 1));
        await page.waitForFunction(() => depager.stats().state === 'applying');
        await page.evaluate(() => resolveApplied());
        assert.deepEqual(await page.evaluate(() => finishPending()), { status: 'ok', page: 1, added: 1 });
        assert.equal(await page.locator('#t100').count(), 1);
      } finally { await context.close(); }
    });

    await t.test('a board root detached during application is never committed', async () => {
      const { context, page } = await setup({ apply: 'detach' });
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => deliver());
        assert.deepEqual(await page.evaluate(() => finishPending()), { status: 'cancelled' });
        const stats = await page.evaluate(() => depager.stats());
        assert.equal(stats.pages, 0);
        assert.equal(stats.nextPage, 1);
        assert.equal(stats.busy, false);
        assert.equal(stats.state, 'paused');
      } finally { await context.close(); }
    });

    await t.test('feature application cannot commit a partial candidate page', async () => {
      const { context, page } = await setup({ apply: 'remove-candidate' });
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => deliver());
        assert.deepEqual(await page.evaluate(() => finishPending()), { status: 'invalid-snapshot' });
        assert.equal(await page.locator('#t100,.depageNumber').count(), 0);
        const stats = await page.evaluate(() => depager.stats());
        assert.equal(stats.pages, 0);
        assert.equal(stats.nextPage, 1);
        assert.equal(stats.state, 'error');
      } finally { await context.close(); }
    });

    await t.test('a rejected apply callback after root detachment releases the loading status', async () => {
      const { context, page } = await setup({ apply: 'detach-reject' });
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => deliver());
        assert.deepEqual(await page.evaluate(() => finishPending()), { status: 'cancelled' });
        const stats = await page.evaluate(() => depager.stats());
        assert.equal(stats.pages, 0);
        assert.equal(stats.nextPage, 1);
        assert.equal(stats.busy, false);
        assert.equal(stats.state, 'paused');
        assert.equal(await page.evaluate(() => states.at(-1).state), 'paused');
        assert.equal(await page.locator('#t100,.depageNumber').count(), 0);
        assert.equal(await page.locator('nav.pages [rel="next"]').count(), 1);
      } finally { await context.close(); }
    });

    await t.test('disableAll removes only owned pages and a null initial next page performs no request', async () => {
      const { context, page } = await setup();
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => deliver()); await page.evaluate(() => finishPending());
        await page.evaluate(() => { config.disableAll = true; depager.refresh(); });
        assert.equal(await page.locator('#t100,.depageNumber').count(), 0);
        assert.equal(await page.evaluate(() => document.getElementById('t10') === originalThread), true);
        assert.equal(await page.locator('#owned-draft').inputValue(), 'preserve-me');
        assert.equal((await page.evaluate(() => depager.stats())).pages, 0);
      } finally { await context.close(); }

      const noNext = await setup({ nextPage: null });
      try {
        assert.deepEqual(await noNext.page.evaluate(() => depager.loadMore()), { status: 'complete' });
        assert.equal(await noNext.page.evaluate(() => loads.length), 0);
      } finally { await noNext.context.close(); }
    });

    await t.test('aggregate page budget stops the loop without discarding an already committed page', async () => {
      const snapshot = parsedPage(1, 2);
      const { context, page } = await setup({ snapshot, limits: { pages: 1 } });
      try {
        await page.evaluate(() => begin()); await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => deliver());
        assert.deepEqual(await page.evaluate(() => finishPending()), { status: 'ok', page: 1, added: 1 });
        assert.equal(await page.locator('#t100').count(), 1);
        assert.deepEqual(await page.evaluate(() => depager.loadMore()), { status: 'limit' });
        assert.equal(await page.evaluate(() => loads.length), 1);
        assert.equal(await page.locator('#t100').count(), 1);
        assert.equal((await page.evaluate(() => depager.stats())).pages, 1);
      } finally { await context.close(); }
    });

    await t.test('alwaysDepage defaults off and opt-in auto loading uses the bounded bottom threshold', async () => {
      const manual = await setup();
      try {
        await manual.page.waitForTimeout(25);
        assert.equal(await manual.page.evaluate(() => loads.length), 0);
      } finally { await manual.context.close(); }

      const automatic = await setup({ config: { alwaysDepage: true }, snapshot: parsedPage(1, null) });
      try {
        await automatic.page.waitForFunction(() => loads.length === 1);
        await automatic.page.evaluate(() => deliver());
        await automatic.page.waitForFunction(() => !depager.stats().busy);
        assert.equal(await automatic.page.locator('#t100').count(), 1);
        assert.equal((await automatic.page.evaluate(() => depager.stats())).auto, true);
      } finally { await automatic.context.close(); }
    });
  } finally { await browser.close(); }
});
