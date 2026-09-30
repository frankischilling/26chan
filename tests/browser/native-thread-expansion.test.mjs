import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { parseUpdaterSnapshot } from '../../apps/public/client/native-updater-snapshot.js';
import { planThreadExpansion } from '../../apps/public/client/native-thread-expansion.js';

const origin = 'https://expansion.example', board = 'demo', thread = '9007199254740992';
const ids = Array.from({ length: 8 }, (_, index) => String(BigInt(thread) + BigInt(index)));
const context = { origin, board, thread, mediaOrigin: '' };
const post = (no, parent = thread, text = 'Owned &lt;script&gt;literal&lt;/script&gt;') => {
  const type = no === parent ? 'op' : 'reply';
  return `<article class="postContainer ${type}Container" id="pc${no}"><div class="post ${type}" id="p${no}">`
    + `<div class="postInfo" id="pi${no}"><span class="name">Anonymous</span><a class="postNum" href="/demo/thread/${parent}#p${no}">No.${no}</a></div>`
    + `<blockquote class="postMessage" id="m${no}">${text}</blockquote><details class="postActions"><summary>Delete or report</summary>`
    + `<form method="post" action="/demo/delete"><input type="hidden" name="no" value="${no}"><label for="delete${no}">Deletion password</label>`
    + `<input type="password" name="password" id="delete${no}" autocomplete="off" required><button>Delete post</button></form></details></div></article>`;
};
const wire = () => ({ version: 2, board, thread, closed: false, archived: false, sticky: false,
  replies: 7, images: 0, posts: ids.map(no => ({ no, file_deleted: false, html: post(no) })), tail_size: 0, tail_id: null });
const parsed = () => {
  const result = parseUpdaterSnapshot(JSON.stringify(wire()), context);
  assert.equal(result.status, 'ok'); return result.snapshot;
};

test('expansion selects only omitted older replies with exact adjacent 64-bit IDs', () => {
  const snapshot = parsed();
  const plan = planThreadExpansion(snapshot, context, [thread, ...ids.slice(3)]);
  assert.deepEqual(plan.additions.map(post => post.no), ids.slice(1, 3));
  assert.equal(plan.cost.posts, 2); assert.ok(plan.cost.nodes > 10 && plan.cost.bytes > 100);
  assert.deepEqual(planThreadExpansion(snapshot, context, [thread]).additions.map(post => post.no), ids.slice(1));
  assert.equal(planThreadExpansion(snapshot, context, [thread, ids[1]]).additions.length, 0);
});

test('partial snapshots and ambiguous original rows never produce an insertion plan', () => {
  for (const original of [[], ids, [ids[1]], [thread, ids[2], ids[1]], [thread, ids[1], ids[1]], [thread, 9007199254740993]]) {
    assert.throws(() => planThreadExpansion(parsed(), context, original));
  }
  for (const change of [value => { value.tail_id = ids[1]; }, value => { value.board = 'other'; },
    value => { value.posts.reverse(); }, value => { value.posts[1].no = value.posts[0].no; },
    value => { value.posts.pop(); }, value => { value.posts[1].tree.children.push({ tag: 'img', attrs: { src: 'https://tracker.example/' }, children: [] }); }]) {
    const snapshot = parsed(); change(snapshot);
    assert.throws(() => planThreadExpansion(snapshot, context, [thread, ...ids.slice(3)]));
  }
});

test('isolated expansion DOM and real parser-worker behavior', async t => {
  const assets = {};
  for (const name of ['native-thread-controls.v1.js', 'native-filter.v1.js', 'native-backlinks.v1.js']) {
    assets[`/static/${name}`] = await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8');
  }
  const css = await readFile(new URL('../../apps/public/static/board.css', import.meta.url), 'utf8');
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup({ real = false, snapshot = parsed(), config = {}, limits = {}, apply = 'ok', two = false } = {}) {
      const browserContext = await browser.newContext();
      const page = await browserContext.newPage(), requests = [], errors = [];
      page.on('pageerror', error => errors.push(error.message));
      const section = `<section class="thread" id="t${thread}">${[thread, ...ids.slice(3)].map(no => post(no)).join('')}`
        + `<p class="omitted">2 posts omitted. <a href="/demo/thread/${thread}">View thread</a></p></section>`;
      const second = two ? '<section class="thread" id="t200"><p class="omitted">1 post omitted. <a href="/demo/thread/200">View thread</a></p></section>' : '';
      await browserContext.route('**/*', async route => {
        const url = new URL(route.request().url());
        if (url.origin === origin && assets[url.pathname]) await route.fulfill({ contentType: 'text/javascript', body: assets[url.pathname] });
        else if (url.href === `${origin}/demo/`) await route.fulfill({ contentType: 'text/html', body:
          `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><style>${css}</style></head><body><main class="board">${section}${second}</main></body></html>` });
        else if (url.pathname === '/favicon.ico') await route.fulfill({ status: 204 });
        else if (url.href === `${origin}/_watch/demo/thread/${thread}/posts`) {
          requests.push({ url: url.href, headers: await route.request().allHeaders() });
          await route.fulfill({ contentType: 'application/json', body: JSON.stringify(wire()) });
        } else { requests.push({ unexpected: url.href }); await route.abort(); }
      });
      await page.goto(`${origin}/demo/`);
      await page.evaluate(async ({ real, snapshot, config, limits, apply, thread }) => {
        const module = await import('/static/native-thread-controls.v1.js');
        window.api = await import('/static/native-filter.v1.js');
        window.projection = (await import('/static/native-backlinks.v1.js')).createCommentProjection();
        window.config = config; window.loads = []; window.cancels = 0; window.applies = 0;
        const root = document.querySelector('.board');
        window.original = [...root.querySelectorAll('.postContainer')];
        window.expansion = module.mountNativeThreadExpansion({ root, board: 'demo', thread: null,
          settings: () => config, projection, limits,
          applied: () => { window.applies++; if (apply === 'throw') throw Error('owned failure'); if (apply === 'hang') return new Promise(() => {}); },
          ...(real ? {} : { createTransport: () => ({ refresh({ signal }) {
            return new Promise(resolve => { window.loads.push({ signal, resolve }); });
          }, cancel() { window.cancels++; } }) }),
        });
        window.deliver = (index = 0) => loads[index].resolve({ status: 'ok', snapshot });
        window.click = () => document.querySelector('.nativeThreadExpand').click();
      }, { real, snapshot, config, limits, apply, thread });
      return { context: browserContext, page, requests, errors };
    }
    const begin = async page => { await page.getByRole('button', { name: `Expand thread ${thread}`, exact: true }).click(); await page.waitForFunction(() => loads.length === 1); };
    const finish = async page => {
      await page.evaluate(() => deliver());
      await page.waitForFunction(() => !expansion.stats().busy);
    };

    await t.test('real fetched snapshots cross the actual worker and preserve visible nodes and drafts', async () => {
      const { context, page, requests, errors } = await setup({ real: true });
      try {
        await context.addCookies([{ name: 'private-session', value: 'synthetic-only', url: origin }]);
        await page.locator(`#pc${ids[7]} details`).evaluate(element => { element.open = true; });
        await page.locator(`#delete${ids[7]}`).fill('owned-tail-draft');
        await page.getByRole('button', { name: `Expand thread ${thread}`, exact: true }).click();
        await page.waitForFunction(() => document.querySelectorAll('.rExpanded').length === 2 && !expansion.stats().busy);
        assert.deepEqual(await page.locator('.thread > .postContainer').evaluateAll(nodes => nodes.map(node => node.id.slice(2))), ids);
        assert.equal(await page.evaluate(() => original.every(element => document.getElementById(element.id) === element)), true);
        assert.equal(await page.locator(`#delete${ids[7]}`).inputValue(), 'owned-tail-draft');
        assert.equal(await page.locator(`#m${ids[1]}`).textContent(), 'Owned <script>literal</script>');
        assert.equal(await page.locator('.postMessage script').count(), 0);
        assert.equal(requests.length, 1); assert.equal(requests[0].headers.cookie, undefined);
        await page.getByRole('button', { name: `Collapse thread ${thread}`, exact: true }).click();
        assert.equal(await page.locator('.rExpanded:visible').count(), 0);
        await page.getByRole('button', { name: `Expand thread ${thread}`, exact: true }).click();
        assert.equal(await page.locator('.rExpanded:visible').count(), 2); assert.equal(requests.length, 1);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('owned expansion classes stay out of local quote source and disabled mode restores original content', async () => {
      const { context, page, errors } = await setup();
      try {
        await begin(page); await finish(page);
        const classes = await page.evaluate(({ id, thread }) => api.localQuoteTree(document.getElementById(`pc${id}`),
          { origin: location.origin, board: 'demo', thread, mediaOrigin: '' }, id, projection).attrs.class, { id: ids[1], thread });
        assert.equal(classes, 'postContainer replyContainer');
        await page.evaluate(() => { config.threadExpansion = false; expansion.refresh(); });
        assert.equal(await page.locator('.rExpanded,.nativeThreadExpand').count(), 0);
        assert.equal(await page.locator('.omitted').textContent(), '2 posts omitted. View thread');
        assert.equal(await page.evaluate(() => expansion.stats().posts), 0);
        await page.evaluate(() => { config.threadExpansion = true; expansion.refresh(); });
        assert.equal(await page.locator('.nativeThreadExpand').count(), 1);
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('one in-flight request is shared by all thread controls and can be cancelled', async () => {
      const { context, page } = await setup({ two: true });
      try {
        await begin(page);
        await page.getByRole('button', { name: 'Expand thread 200', exact: true }).click();
        assert.equal(await page.evaluate(() => loads.length), 1);
        assert.match(await page.locator('#t200 .nativeExpansionStatus').textContent(), /Another thread/);
        await page.getByRole('button', { name: `Cancel expansion of thread ${thread}`, exact: true }).click();
        assert.equal(await page.evaluate(() => loads[0].signal.aborted), true);
        await page.evaluate(() => deliver());
        assert.equal(await page.locator('.rExpanded').count(), 0);
        assert.equal(await page.evaluate(() => expansion.stats().busy), false);
      } finally { await context.close(); }
    });

    await t.test('aggregate page budgets reject a complete expansion without retaining a prefix', async () => {
      for (const limits of [{ posts: 1 }, { nodes: 1 }, { bytes: 1 }]) {
        const { context, page } = await setup({ limits });
        try {
          await begin(page); await finish(page);
          assert.equal(await page.locator('.rExpanded').count(), 0);
          assert.match(await page.locator('.nativeExpansionStatus').textContent(), /Page expansion limit/);
          assert.equal(await page.evaluate(() => expansion.stats().posts), 0);
          assert.equal(await page.getByRole('link', { name: 'View thread' }).isVisible(), true);
        } finally { await context.close(); }
      }
    });

    await t.test('a duplicate live ID is rejected before any approved image can load', async () => {
      const snapshot = parsed();
      const { context, page, requests } = await setup({ snapshot });
      try {
        await page.evaluate(id => { const collision = document.createElement('div'); collision.id = `delete${id}`; document.body.append(collision); }, ids[1]);
        await begin(page); await finish(page);
        assert.equal(await page.locator('.rExpanded').count(), 0);
        assert.match(await page.locator('.nativeExpansionStatus').textContent(), /Could not apply/);
        assert.deepEqual(requests, []);
      } finally { await context.close(); }
    });

    await t.test('malformed worker recipes cannot create markup, forms or fetches', async () => {
      const snapshot = parsed(); snapshot.posts[1].tree.children.push({ tag: 'img', attrs: { src: 'https://tracker.example/collect' }, children: [] });
      const { context, page, requests } = await setup({ snapshot });
      try {
        await begin(page); await finish(page);
        assert.equal(await page.locator('.rExpanded').count(), 0); assert.deepEqual(requests, []);
        assert.equal(await page.locator('img').count(), 0);
      } finally { await context.close(); }
    });

    await t.test('failed or hung feature application releases the complete expansion', async () => {
      for (const apply of ['throw', 'hang']) {
        const { context, page } = await setup({ apply, limits: { applyMs: 25 } });
        try {
          await begin(page); await finish(page);
          assert.equal(await page.locator('.rExpanded').count(), 0);
          assert.equal(await page.evaluate(() => expansion.stats().posts), 0);
          assert.equal(await page.getByRole('link', { name: 'View thread' }).isVisible(), true);
        } finally { await context.close(); }
      }
    });

    await t.test('page suspension rejects late work and history restoration reattaches one control', async () => {
      const { context, page } = await setup();
      try {
        await begin(page);
        await page.evaluate(() => { dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true })); deliver(); });
        assert.equal(await page.locator('.rExpanded,.nativeThreadExpand').count(), 0);
        await page.evaluate(() => dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })));
        assert.equal(await page.locator('.nativeThreadExpand').count(), 1);
        await page.evaluate(() => expansion.destroy());
        assert.equal(await page.locator('.nativeThreadExpand').count(), 0);
        assert.equal(await page.locator('.omitted').textContent(), '2 posts omitted. View thread');
      } finally { await context.close(); }
    });
  } finally { await browser.close(); }
});
