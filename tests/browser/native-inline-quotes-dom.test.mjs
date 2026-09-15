import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';

const origin = 'https://board.example', mediaOrigin = 'https://media.example';
const quote = (no, id = '', route = `/demo/post/${no}`) => `<a${id ? ` id="${id}"` : ''} class="quotelink" href="${route}">&gt;&gt;${no}</a>`;
function post(no, message, thread = '100') {
  const type = no === thread ? 'op' : 'reply';
  return `<article class="postContainer ${type}Container" id="pc${no}"><div class="post ${type}" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span><a class="postNum" href="/demo/thread/${thread}#p${no}">No.${no}</a></div><blockquote class="postMessage" id="m${no}">${message}</blockquote><details class="postActions"><summary>Delete or report</summary><form method="post" action="/demo/delete"><input type="hidden" name="no" value="${no}"><input type="password" name="password" id="delete${no}" autocomplete="off" required><button>Delete</button></form></details></div></article>`;
}
const envelope = (no = '120', message = `Remote <s>secret</s> ${quote('121')}`, board = 'demo', thread = '100') => ({
  version: 1, board, thread, post: { no, file_deleted: false, html: post(no, message, thread) },
});

test('isolated current-source inline quote DOM contracts', async t => {
  const css = await readFile(new URL('../../apps/public/static/board.css', import.meta.url), 'utf8')
    + await readFile(new URL('../../apps/public/static/themes/common.css', import.meta.url), 'utf8');
  const worker = await readFile(new URL('../../apps/public/static/native-filter.v1.js', import.meta.url), 'utf8');
  const modules = new Map();
  const imports = JSON.stringify({ imports: { parse5: '/node_modules/parse5/dist/index.js',
    entities: '/node_modules/entities/dist/index.js', 'entities/decode': '/node_modules/entities/dist/decode.js',
    'entities/escape': '/node_modules/entities/dist/escape.js' } });
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup({ messages = [['100', quote('101', 'one')], ['101', `Target <s>secret</s> ${quote('102')}`],
      ['102', 'Another target']], mobile = false, mobileLayout = mobile, config = { inlineQuotes: true }, limits = {},
      remote = false, handler, archive = false } = {}) {
      const context = await browser.newContext({ viewport: { width: mobile ? 390 : 1000, height: 800 },
        ...(mobile ? { isMobile: true, hasTouch: true, userAgent: 'Test Mobile browser' } : {}) });
      const page = await context.newPage(), requests = [], errors = [];
      page.on('pageerror', error => errors.push(error.message));
      const fixture = `<!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1"><script type="importmap">${imports}</script><style>${css}</style></head><body><main class="board"><section class="thread" id="t100">${messages.map(([no, message]) => post(no, message)).join('')}</section></main></body></html>`;
      await page.route('**/*', async route => {
        const url = new URL(route.request().url());
        if (url.origin === origin && url.pathname === '/static/native-filter.v1.js') {
          await route.fulfill({ contentType: 'text/javascript', body: worker }); return;
        }
        if (url.origin === origin && url.pathname === '/static/themes/fade.png') {
          await route.fulfill({ status: 204 }); return;
        }
        if (url.origin === origin && /^\/(?:apps\/public\/(?:client|static)\/|node_modules\/(?:parse5|entities)\/dist\/)[a-zA-Z0-9_./-]+\.js$/.test(url.pathname)) {
          if (!modules.has(url.pathname)) modules.set(url.pathname, await readFile(new URL(`../..${url.pathname}`, import.meta.url), 'utf8'));
          await route.fulfill({ contentType: 'text/javascript', body: modules.get(url.pathname) }); return;
        }
        if (url.origin === origin && url.pathname.startsWith('/_watch/')) {
          requests.push({ url: url.href, method: route.request().method(), headers: await route.request().allHeaders() });
          if (handler) await handler(route);
          else await route.fulfill({ contentType: 'application/json', body: JSON.stringify(envelope(url.pathname.split('/').at(-1))) });
          return;
        }
        if (url.origin === origin && url.pathname === '/demo/thread/100') {
          await route.fulfill({ contentType: 'text/html', body: fixture }); return;
        }
        requests.push({ url: url.href }); await route.abort();
      });
      await page.goto(`${origin}/demo/thread/100`);
      await page.evaluate(async ({ config, limits, mobile, mobileLayout, remote, mediaOrigin, archive }) => {
        const inline = await import('/apps/public/client/native-inline-quotes.js');
        const { createCommentProjection } = await import('/apps/public/client/native-comment-projection.js');
        const quotes = await import('/apps/public/client/native-quote-preview.js');
        const transport = await import('/apps/public/client/native-quote-preview-transport.js');
        const { mountNativeBacklinks } = await import('/apps/public/client/native-backlinks.js');
        window.linkification = await import('/apps/public/client/native-linkification.js');
        window.tracked = await import('/apps/public/client/native-tracked-quotes.js');
        window.filters = await import('/apps/public/client/native-page-filters.js');
        const root = document.querySelector('.board');
        window.config = config; window.loads = []; window.cancels = 0; window.previewCancels = 0; window.previewLoads = 0; window.navigation = [];
        window.projection = createCommentProjection(); window.preview = null;
        window.backlinks = mountNativeBacklinks({ root, board: 'demo', thread: '100', origin: location.origin,
          settings: () => window.config, mobile: { matches: mobileLayout }, readNeverMobile: () => null,
          quoteTarget: quotes.quoteTarget, projection, changed: () => window.preview?.refresh() });
        const inlineTransport = remote ? new transport.NativeQuotePreviewTransport({ origin: location.origin, mediaOrigin,
          createWorker: () => new Worker('/static/native-filter.v1.js', { type: 'module' }) }) : {
          load(ref, { signal }) {
            return new Promise(resolve => {
              window.loads.push({ ref, signal, resolve });
              signal.addEventListener('abort', () => resolve({ status: 'cancelled' }), { once: true });
            });
          }, cancel() { window.cancels++; },
        };
        window.inline = inline.mountNativeInlineQuotes({ root, board: 'demo', thread: '100', origin: location.origin, mediaOrigin,
          settings: () => window.config, mobileDevice: mobile, archive, limits, projection, ...quotes,
          checkedQuotePreview: transport.checkedQuotePreview, transport: inlineTransport,
          companion: link => window.preview?.companion(link) ?? backlinks.companion(link),
          backlinkOwner: link => backlinks.backlinkOwner(link), prepareBacklinks: (...args) => backlinks.prepareInlineCopy(...args),
          navigate: href => window.navigation.push(href) });
        window.preview = quotes.mountNativeQuotePreview({ root, board: 'demo', thread: '100', origin: location.origin,
          mediaOrigin, projection, userAgent: mobile ? 'Test Mobile browser' : 'Desktop', settings: () => window.config,
          transport: { load: async () => { window.previewLoads++; return { status: 'unavailable' }; }, cancel() { window.previewCancels++; } },
          companion: link => backlinks.companion(link) ?? window.inline.companion(link),
          quoteContext: link => window.inline.quoteContext(link),
          inlineHoverEligible: link => window.inline.hoverEligible(link),
          arbitrateClick: event => { const outcome = window.inline.click(event); window.lastOutcome = outcome; return outcome; } });
        document.addEventListener('click', event => {
          window.lastClick = { outcome: window.lastOutcome, prevented: event.defaultPrevented, trusted: event.isTrusted };
          // Keep ordinary-navigation checks inside the synthetic fixture.
          if (event.target.closest?.('a')) event.preventDefault();
        });
        window.fire = (selector, options = {}) => {
          document.querySelector(selector).dispatchEvent(new MouseEvent('click', { button: 0, bubbles: true, cancelable: true, ...options }));
          return window.lastClick;
        };
      }, { config, limits, mobile, mobileLayout, remote, mediaOrigin, archive });
      return { context, page, requests, errors };
    }

    await t.test('normal, nested, wrapper and spoiler placement preserves original links and strips nested copies', async () => {
      const { context, page, errors } = await setup({ messages: [
        ['100', `${quote('101', 'one')} ${quote('101', 'two')} <s><s><span class="quote">${quote('102', 'wrapped')}</span></s></s>`],
        ['101', `Target ${quote('102', 'nested')} ${quote('100', 'cycle')} <s>secret</s>`], ['102', 'Leaf'],
      ] });
      try {
        const result = await page.evaluate(() => {
          const original = document.getElementById('one');
          fire('#one');
          const first = original.nextElementSibling;
          fire('#one + .inlined .postMessage a');
          const afterNested = inline.stats().open;
          const cycle = fire('#one + .inlined .postMessage a[href="/demo/post/100"]');
          fire('#two');
          const second = document.getElementById('two').nextElementSibling;
          const nestedStripped = second.querySelectorAll('.inlined').length;
          fire('#wrapped');
          const wrapped = document.getElementById('wrapped').closest('s').parentElement;
          const wrappedPlacement = wrapped.nextElementSibling?.classList.contains('inlined');
          const copies = [...document.querySelectorAll('.inlined')];
          const safe = copies.every(node => !node.querySelector('[id],form,input,button,video,audio,details,.postMenuBtn')
            && !node.classList.contains('reveal-spoilers'));
          fire('#one');
          return { firstReady: first.dataset.inlineState, afterNested, cycle, nestedStripped, wrappedPlacement,
            safe, originalIdentity: original === document.getElementById('one'), remaining: inline.stats().open,
            firstRemoved: !first.isConnected, secondConnected: second.isConnected };
        });
        assert.deepEqual(result, { firstReady: 'ready', afterNested: 2,
          cycle: { outcome: 'inlinehandled', prevented: true, trusted: false }, nestedStripped: 0,
          wrappedPlacement: true, safe: true, originalIdentity: true, remaining: 2, firstRemoved: true, secondConnected: true });
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('desktop backlink copies prepend and reference-count their original without creating graph edges', async () => {
      const { context, page } = await setup({ messages: [['100', 'Opening'], ['101', 'Other'],
        ['102', `${quote('100')} ${quote('101')}`]] });
      try {
        const result = await page.evaluate(() => {
          const before = projection.queryAll(document.querySelector('.board'), '.backlink a.quotelink').length;
          fire('#bl_100 a.quotelink'); fire('#bl_101 a.quotelink');
          const target = document.getElementById('pc102');
          const count = target.getAttribute('data-inline-count'), display = target.style.display;
          backlinks.refresh();
          const after = projection.queryAll(document.querySelector('.board'), '.backlink a.quotelink').length;
          const prepended = ['100', '101'].every(no => document.getElementById('m' + no).firstElementChild?.classList.contains('inlined'));
          fire('#bl_100 a.quotelink'); const one = target.getAttribute('data-inline-count');
          fire('#bl_101 a.quotelink');
          return { before, after, count, display, prepended, one, restored: target.style.display,
            attr: target.getAttribute('data-inline-count'), stats: inline.stats() };
        });
        assert.deepEqual(result, { before: 2, after: 2, count: '2', display: 'none', prepended: true, one: '1',
          restored: '', attr: null, stats: { open: 0, pending: 0, nodes: 0, characters: 0, hidden: 0 } });
      } finally { await context.close(); }
    });

    await t.test('one click arbiter retains modifiers, direct-self navigation, exact target and disabled defaults', async () => {
      const { context, page } = await setup({ messages: [['100', `${quote('101', 'one')} ${quote('100', 'self')} <a id="child" class="quotelink" href="/demo/post/101"><span id="label">&gt;&gt;101</span></a>`], ['101', 'Target']] });
      try {
        const result = await page.evaluate(() => {
          const modified = ['ctrlKey', 'metaKey', 'altKey'].map(key => { const value = fire('#one', { [key]: true }); inline.clear(); return value; });
          const shift = fire('#one', { shiftKey: true }), self = fire('#self'), child = fire('#label');
          document.getElementById('self').href = '/demo/thread/90#p100'; const selfAlias = fire('#self');
          const secondary = fire('#one', { button: 1 });
          config = {}; const defaultOff = fire('#one'); config = { inlineQuotes: true, disableAll: true }; const disabled = fire('#one');
          config = { inlineQuotes: true, quotePreview: false }; const previewDisabled = fire('#one');
          const previewDisabledOpen = inline.stats().open; inline.clear();
          config = { inlineQuotes: true }; document.getElementById('one').href = 'https://evil.example/demo/post/101';
          const invalid = fire('#one');
          return { modified, shift, self, selfAlias, child, secondary, defaultOff, disabled,
            previewDisabled, previewDisabledOpen, invalid, navigation, open: inline.stats().open };
        });
        assert.equal(result.modified.every(value => value.prevented && value.outcome === 'inlinehandled'), true);
        assert.equal(result.shift.outcome, 'ordinarynav'); assert.equal(result.shift.prevented, true);
        assert.deepEqual(result.navigation, [`${origin}/demo/post/101`]);
        for (const name of ['self', 'selfAlias', 'child', 'secondary', 'defaultOff', 'disabled', 'invalid']) assert.equal(result[name].prevented, false, name);
        assert.deepEqual(result.previewDisabled, { outcome: 'inlinehandled', prevented: true, trusted: false });
        assert.equal(result.previewDisabledOpen, 1);
        assert.equal(result.open, 0);
      } finally { await context.close(); }
    });

    await t.test('trusted mobile taps keep # adjacency and inline precedence over compatibility hover', async () => {
      const { context, page, errors } = await setup({ mobile: true });
      try {
        await page.locator('#one').tap();
        const opened = await page.evaluate(() => {
          const source = document.getElementById('one');
          return { trusted: lastClick.trusted, outcome: lastClick.outcome, pair: source.nextElementSibling.className,
            inline: source.nextElementSibling.nextElementSibling.dataset.inlineState, count: inline.stats().open };
        });
        assert.deepEqual(opened, { trusted: true, outcome: 'inlinehandled', pair: 'quoteLink', inline: 'ready', count: 1 });
        await page.locator('#one + .quoteLink').tap();
        assert.equal(await page.evaluate(() => lastClick.prevented), false);
        await page.locator('#one').tap();
        assert.equal(await page.evaluate(() => inline.stats().open), 0);
        const previewResult = await page.evaluate(() => {
          config = { inlineQuotes: false, quotePreview: true }; document.getElementById('p101').style.marginTop = '1300px';
          fire('#one');
          // Synthetic mouseout after a tap-owned mobile preview must retain it.
          document.getElementById('one').dispatchEvent(new MouseEvent('mouseout', { bubbles: true, relatedTarget: document.body }));
          return !!document.getElementById('quote-preview');
        });
        assert.equal(previewResult, true); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('trusted remote mobile tap starts only the inline one-post transport', async () => {
      const { context, page, errors } = await setup({ mobile: true, messages: [['100', quote('120', 'one')]] });
      try {
        const eligibility = await page.evaluate(() => {
          const link = document.getElementById('one'), before = { stats: inline.stats(), html: link.outerHTML,
            inlineLoads: loads.length, previewLoads };
          const eligible = inline.hoverEligible(link);
          return { eligible, before, after: { stats: inline.stats(), html: link.outerHTML,
            inlineLoads: loads.length, previewLoads } };
        });
        assert.equal(eligibility.eligible, true);
        assert.deepEqual(eligibility.after, eligibility.before, 'hover eligibility must be read-only');
        await page.locator('#one').tap();
        await page.waitForFunction(() => loads.length === 1);
        const result = await page.evaluate(() => ({ previewLoads, inlineLoads: loads.length,
          post: loads[0].ref.post, trusted: lastClick.trusted, outcome: lastClick.outcome,
          pending: inline.stats().pending }));
        assert.deepEqual(result, { previewLoads: 0, inlineLoads: 1, post: '120', trusted: true,
          outcome: 'inlinehandled', pending: 1 });
        assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('mobile backlink placement stays adjacent and never hides the original', async () => {
      const { context, page } = await setup({ mobile: true, messages: [['100', 'Opening'], ['101', 'Owner'], ['102', quote('101')]] });
      try {
        const result = await page.evaluate(() => {
          fire('#bl_101 a.quotelink');
          const link = document.querySelector('#bl_101 a.quotelink');
          return { pair: link.nextElementSibling.className, inline: link.nextElementSibling.nextElementSibling.dataset.inlineState,
            display: document.getElementById('pc102').style.display, hidden: inline.stats().hidden };
        });
        assert.deepEqual(result, { pair: 'quoteLink', inline: 'ready', display: '', hidden: 0 });
      } finally { await context.close(); }
    });

    await t.test('queued remote requests ignore repeated pending clicks and keep preview cancellation separate', async () => {
      const { context, page } = await setup({ messages: [['100', `${quote('120', 'one')} ${quote('121', 'two')}`]] });
      try {
        await page.evaluate(() => { fire('#one'); fire('#one'); fire('#two'); });
        await page.waitForFunction(() => loads.length === 1);
        assert.deepEqual(await page.evaluate(() => ({ count: loads.length, pending: inline.stats().pending,
          first: loads[0].ref.post, canceled: loads[0].signal.aborted, repeat: document.querySelectorAll('.inlined').length })),
        { count: 1, pending: 2, first: '120', canceled: false, repeat: 2 });
        await page.evaluate(() => { preview.clear(); loads[0].resolve({ status: 'http-error', httpStatus: 404 }); });
        await page.waitForFunction(() => loads.length === 2);
        const unavailable = await page.evaluate(() => ({ state: document.querySelector('#one + .inlined').dataset.inlineState,
          text: document.querySelector('#one + .inlined').textContent, second: loads[1].ref.post }));
        assert.deepEqual(unavailable, { state: 'unavailable', text: 'This post or thread is unavailable.', second: '121' });
        await page.evaluate(() => { fire('#one'); fire('#one'); });
        assert.equal(await page.evaluate(() => loads.length), 2);
        await page.evaluate(() => { inline.clear(); });
        assert.deepEqual(await page.evaluate(() => ({ canceled: loads[1].signal.aborted, stats: inline.stats(), nodes: document.querySelectorAll('.inlined').length })),
          { canceled: true, stats: { open: 0, pending: 0, nodes: 0, characters: 0, hidden: 0 }, nodes: 0 });
      } finally { await context.close(); }
    });

    await t.test('queue wait is included in the total deadline and releases every pending request', async () => {
      const { context, page } = await setup({ limits: { pendingMs: 100 }, messages: [['100', `${quote('120', 'one')} ${quote('121', 'two')}`]] });
      try {
        await page.evaluate(() => { fire('#one'); fire('#two'); });
        await page.waitForFunction(() => inline.stats().pending === 0);
        const result = await page.evaluate(() => ({ states: [...document.querySelectorAll('.inlined')].map(node => node.dataset.inlineState),
          aborted: loads.every(item => item.signal.aborted), nodes: inline.stats().nodes }));
        assert.deepEqual(result, { states: ['error', 'error'], aborted: true, nodes: 4 });
      } finally { await context.close(); }
    });

    await t.test('real disposable parser worker validates remote one-post responses without credentials', async () => {
      const { context, page, requests, errors } = await setup({ remote: true, messages: [['100', quote('120', 'one')]] });
      try {
        await context.addCookies([{ name: 'fixture', value: 'private', url: origin }]);
        await page.evaluate(() => fire('#one'));
        await page.waitForFunction(() => document.querySelector('.inlined')?.dataset.inlineState === 'ready');
        const result = await page.evaluate(() => ({ text: document.querySelector('.inlined').textContent,
          controls: document.querySelector('.inlined').querySelectorAll('form,input,button,[id],video,audio').length,
          state: inline.stats().pending }));
        assert.match(result.text, /Remote/); assert.equal(result.controls, 0); assert.equal(result.state, 0);
        assert.equal(requests.length, 1, JSON.stringify(requests)); assert.equal(requests[0].headers.cookie, undefined);
        assert.equal(requests[0].url, `${origin}/_watch/demo/post/120`); assert.deepEqual(errors, []);
      } finally { await context.close(); }
    });

    await t.test('atomic aggregate rejection creates no copied elements and open/pending limits preserve ordinary navigation', async () => {
      const { context, page } = await setup({ limits: { nodes: 1 } });
      try {
        const result = await page.evaluate(() => {
          const create = document.createElement.bind(document); let built = 0;
          document.createElement = (...args) => { built++; return create(...args); };
          const click = fire('#one'); document.createElement = create;
          return { built, prevented: click.prevented, stats: inline.stats() };
        });
        assert.deepEqual(result, { built: 0, prevented: false, stats: { open: 0, pending: 0, nodes: 0, characters: 0, hidden: 0 } });
      } finally { await context.close(); }
      const limited = await setup({ limits: { open: 2, pending: 1 }, messages: [['100', `${quote('120', 'one')} ${quote('121', 'two')} ${quote('101', 'local')} ${quote('101', 'extra')}`], ['101', 'Local']] });
      try {
        const result = await limited.page.evaluate(() => {
          fire('#one'); const pendingLimit = fire('#two'); fire('#local'); const openLimit = fire('#extra');
          return { pending: pendingLimit.prevented, open: openLimit.prevented, stats: inline.stats().open };
        });
        assert.deepEqual(result, { pending: false, open: false, stats: 2 });
      } finally { await limited.context.close(); }
    });

    await t.test('original serialization, filters, labels and linkification ignore only owned projections', async () => {
      const { context, page } = await setup({ messages: [['100', 'Opening'], ['101', `${quote('102', 'one')} https://original.example/path`],
        ['102', `COPIED-NOISE ${quote('100')} <span class="spoiler">masked</span>`]] });
      try {
        const result = await page.evaluate(async () => {
          const message = document.getElementById('m101'), before = backlinks.commentHTML(message);
          fire('#one'); const copy = document.querySelector('#one + .inlined');
          const after = backlinks.commentHTML(message);
          const clone = projection.clone(message);
          const cloneOriginalAttributes = clone.querySelector('a').className === 'quotelink'
            && !clone.querySelector('a').hasAttribute('aria-expanded');
          const filename = document.createElement('p'); filename.className = 'file';
          filename.textContent = 'COPIED-FILENAME'; copy.append(filename);
          const original = projection.text(message, ' '), post = document.getElementById('p101');
          const containsCopied = !!projection.query(post, '.file');
          const sourceLink = document.getElementById('one');
          linkification.linkifyMessage(message, projection);
          const linked = projection.query(message, 'a.linkified')?.textContent;
          const copyText = copy.textContent;
          tracked.markNativeTrackedQuotes(document.getElementById('t100'), new Set(['100']), true, { ...backlinks, projection });
          const copiedTracked = copy.querySelectorAll('.ql-tracked').length;
          let rows = [];
          const mounted = filters.mountNativeFilters({ board: 'demo', threadId: '100', settings: () => ({ filter: true }),
            read: () => '[]', save: async () => true, getTracked: () => new Set(), changed: () => {}, projection,
            commentHTML: message => backlinks.commentHTML(message), match: async (_rules, _board, values) => {
              rows = values; return { status: 'ok', matches: [] };
            } });
          await mounted.refresh();
          const row = rows.find(row => row.no === '101');
          const forged = document.createElement('span'); forged.className = 'inlined'; forged.textContent = 'FORGED-LITERAL'; message.append(forged);
          const forgedRetained = backlinks.commentHTML(message).includes('FORGED-LITERAL');
          return { same: before === after, cloneOriginalAttributes, original, containsCopied, linked, sourceSame: sourceLink === document.getElementById('one'),
            copyUnchanged: copyText === copy.textContent, copiedTracked, filename: row.filename,
            filteredNoise: row.com.includes('COPIED-NOISE'), forgedRetained };
        });
        assert.equal(result.same, true); assert.equal(result.cloneOriginalAttributes, true); assert.equal(result.original.includes('COPIED-NOISE'), false);
        assert.equal(result.containsCopied, false); assert.equal(result.linked, 'https://original.example/path');
        assert.equal(result.sourceSame, true); assert.equal(result.copyUnchanged, true); assert.equal(result.copiedTracked, 0);
        assert.equal(result.filename, ''); assert.equal(result.filteredNoise, false); assert.equal(result.forgedRetained, true);
      } finally { await context.close(); }
    });

    await t.test('settings, source removal and synthetic BFCache events release descendants and hidden originals', async () => {
      const { context, page } = await setup({ messages: [['100', 'Opening'], ['101', quote('100')]] });
      try {
        const result = await page.evaluate(() => {
          fire('#bl_100 a.quotelink');
          window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
          const hiddenClean = document.getElementById('pc101').style.display === '' && inline.stats().open === 0;
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
          fire('#bl_100 a.quotelink'); const restored = inline.stats().open;
          config = { inlineQuotes: false }; window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
          const disabled = inline.stats().open;
          config = { inlineQuotes: true }; document.dispatchEvent(new Event('4chanSettingsSaved'));
          fire('#bl_100 a.quotelink'); document.getElementById('pc101').remove(); inline.refresh();
          return { hiddenClean, restored, disabled, removed: inline.stats().open };
        });
        assert.deepEqual(result, { hiddenClean: true, restored: 1, disabled: 0, removed: 0 });
      } finally { await context.close(); }
    });

    await t.test('copying an original with an open inline removes that projection and its presentation state', async () => {
      const { context, page } = await setup({ messages: [['100', quote('101', 'one')],
        ['101', quote('102', 'nested')], ['102', 'Only in original inline']] });
      try {
        const result = await page.evaluate(() => {
          fire('#nested'); fire('#one');
          const copy = document.querySelector('#one + .inlined');
          return { open: inline.stats().open, nested: copy.querySelectorAll('.inlined').length,
            state: copy.querySelector('a.quotelink').className, originalOpen: document.getElementById('nested').classList.contains('linkfade'),
            inheritedText: copy.textContent.includes('Only in original inline') };
        });
        assert.deepEqual(result, { open: 2, nested: 0, state: 'quotelink', originalOpen: true, inheritedText: false });
      } finally { await context.close(); }
    });

    await t.test('eight levels of nesting admit atomically and closing the ancestor reclaims the full tree', async () => {
      const messages = Array.from({ length: 11 }, (_, index) => [String(100 + index), quote(String(101 + index), index === 0 ? 'one' : '')]);
      const { context, page } = await setup({ messages });
      try {
        const result = await page.evaluate(() => {
          let anchor = document.getElementById('one'), ninth;
          for (let index = 0; index < 9; index++) {
            anchor.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 }));
            if (index === 8) ninth = lastClick;
            else anchor = anchor.nextElementSibling.querySelector(':scope > .postMessage > a.quotelink');
          }
          const count = inline.stats().open;
          fire('#one');
          return { count, ninth, after: inline.stats() };
        });
        assert.deepEqual(result, { count: 8, ninth: { outcome: 'ordinarynav', prevented: false, trusted: false },
          after: { open: 0, pending: 0, nodes: 0, characters: 0, hidden: 0 } });
      } finally { await context.close(); }
    });

    await t.test('board and thread collisions never borrow local DOM and positive i64 limits stay exact', async () => {
      const { context, page } = await setup({ messages: [['100', `${quote('101', 'other', '/other/post/101')} ${quote('101', 'wrong', '/demo/thread/90#p101')} ${quote('9223372036854775807', 'maximum')} ${quote('9223372036854775808', 'overflow')}`], ['101', 'LOCAL-COLLISION']] });
      try {
        await page.evaluate(() => { fire('#other'); fire('#wrong'); fire('#maximum'); });
        await page.waitForFunction(() => loads.length === 1);
        const result = await page.evaluate(() => {
          const overflow = fire('#overflow');
          return { pending: inline.stats().pending, localBorrowed: [...document.querySelectorAll('.inlined')].some(node => node.textContent.includes('LOCAL-COLLISION')),
            first: loads[0].ref, overflow };
        });
        assert.deepEqual(result, { pending: 3, localBorrowed: false, first: { board: 'other', post: '101', thread: null },
          overflow: { outcome: 'ordinarynav', prevented: false, trusted: false } });
        await page.evaluate(() => loads[0].resolve({ status: 'http-error', httpStatus: 404 }));
        await page.waitForFunction(() => loads.length === 2);
        assert.deepEqual(await page.evaluate(() => loads[1].ref), { board: 'demo', post: '101', thread: '90' });
        await page.evaluate(() => loads[1].resolve({ status: 'http-error', httpStatus: 404 }));
        await page.waitForFunction(() => loads.length === 3);
        assert.equal(await page.evaluate(() => loads[2].ref.post), '9223372036854775807');
      } finally { await context.close(); }
    });

    await t.test('source invalidation cancels its slot and rejects a late malformed response without affecting the next source', async () => {
      const { context, page } = await setup({ messages: [['100', `${quote('120', 'one')} ${quote('121', 'two')}`]] });
      try {
        await page.evaluate(() => { fire('#one'); fire('#two'); });
        await page.waitForFunction(() => loads.length === 1);
        await page.evaluate(() => { document.getElementById('one').href = '/demo/post/122'; inline.refresh(); });
        await page.waitForFunction(() => loads.length === 2);
        assert.equal(await page.evaluate(() => loads[0].signal.aborted), true);
        await page.evaluate(() => { loads[0].resolve({ status: 'ok', snapshot: {} }); loads[1].resolve({ status: 'ok', snapshot: {} }); });
        await page.waitForFunction(() => document.querySelector('#two + .inlined')?.dataset.inlineState === 'error');
        const result = await page.evaluate(() => ({ count: inline.stats().open, pending: inline.stats().pending,
          stale: document.querySelector('#one + .inlined')?.textContent ?? null,
          error: document.querySelector('#two + .inlined').textContent }));
        assert.deepEqual(result, { count: 1, pending: 0, stale: null, error: 'Error: Quote could not be loaded.' });
      } finally { await context.close(); }
    });

    await t.test('aggregate text admission and per-post text/depth bounds do not partially construct a rejected copy', async () => {
      const { context, page } = await setup({ limits: { characters: 1500 }, messages: [['100', `${quote('101', 'one')} ${quote('101', 'two')}`], ['101', 'x'.repeat(600)]] });
      try {
        const result = await page.evaluate(() => {
          fire('#one');
          const before = inline.stats(), create = document.createElement.bind(document); let built = 0;
          document.createElement = (...args) => { built++; return create(...args); };
          const rejected = fire('#two'); document.createElement = create;
          return { before: before.open, built, prevented: rejected.prevented, unchanged: JSON.stringify(before) === JSON.stringify(inline.stats()) };
        });
        assert.deepEqual(result, { before: 1, built: 0, prevented: false, unchanged: true });
      } finally { await context.close(); }
      const bounded = await setup();
      try {
        const results = await bounded.page.evaluate(() => {
          const target = document.getElementById('m101');
          target.textContent = 'x'.repeat(262144); const text = fire('#one');
          target.textContent = ''; let parent = target;
          for (let index = 0; index < 33; index++) { const span = document.createElement('span'); parent.append(span); parent = span; }
          parent.textContent = 'deep'; const depth = fire('#one');
          return { text: text.prevented, depth: depth.prevented, open: inline.stats().open };
        });
        assert.deepEqual(results, { text: false, depth: false, open: 0 });
      } finally { await bounded.context.close(); }
    });

    await t.test('unsafe controls and media are omitted without reading filled credentials or cloning the original post', async () => {
      const { context, page } = await setup();
      try {
        const result = await page.evaluate(() => {
          const original = document.getElementById('p101'), article = original.parentElement;
          original.cloneNode = article.cloneNode = () => { throw new Error('Original post cloning is forbidden'); };
          const password = document.getElementById('delete101'); password.value = 'SECRET-FIXTURE';
          password.getAttribute = () => { throw new Error('Credential attributes must not be read'); };
          Object.defineProperty(password, 'value', { get() { throw new Error('Credential values must not be read'); } });
          const video = document.createElement('video'); video.setAttribute('autoplay', ''); original.append(video);
          const image = document.createElement('img'); image.setAttribute('src', 'data:image/svg+xml,unsafe'); image.setAttribute('srcset', 'https://tracker.example/x 2x'); original.append(image);
          const safe = document.createElement('img'); safe.setAttribute('src', 'https://media.example/demo/1s.jpg'); safe.setAttribute('alt', 'safe');
          safe.setAttribute('srcset', 'https://tracker.example/y 2x'); original.append(safe);
          fire('#one'); const copy = document.querySelector('#one + .inlined');
          return { ready: copy?.dataset.inlineState, forbidden: copy?.querySelectorAll('form,input,button,video,audio,[id],[srcset]').length,
            secret: copy?.textContent.includes('SECRET-FIXTURE'), images: [...copy.querySelectorAll('img')].map(node => node.getAttribute('src')) };
        });
        assert.deepEqual(result, { ready: 'ready', forbidden: 0, secret: false, images: ['https://media.example/demo/1s.jpg'] });
      } finally { await context.close(); }
    });

    await t.test('local backlink metadata is admitted inside the per-post node ceiling', async () => {
      const { context, page } = await setup();
      try {
        const result = await page.evaluate(async () => {
          const quotes = await import('/apps/public/client/native-quote-preview.js');
          const message = document.getElementById('m101'), fragment = document.createDocumentFragment();
          for (let index = 0; index < 16374; index++) fragment.append(document.createElement('wbr'));
          message.replaceChildren(fragment);
          const input = { origin: location.origin, mediaOrigin: 'https://media.example', board: 'demo', thread: '100' };
          const plan = quotes.prepareQuotePost(quotes.localQuoteTree(document.getElementById('pc101'), input, '101', projection), input, '101');
          const metadata = backlinks.prepareInlineCopy(document.getElementById('p101'), document.getElementById('one'));
          const clicked = fire('#one');
          return { recipeFits: plan.nodes <= 16384, combinedExceeds: plan.nodes + metadata.nodes > 16384,
            prevented: clicked.prevented, open: inline.stats().open };
        });
        assert.deepEqual(result, { recipeFits: true, combinedExceeds: true, prevented: false, open: 0 });
      } finally { await context.close(); }
    });

    await t.test('unavailable settings clear existing copies and archive pages leave inline navigation untouched', async () => {
      const { context, page } = await setup();
      try {
        const result = await page.evaluate(() => {
          fire('#one');
          Object.defineProperty(window, 'config', { configurable: true, get() { throw new Error('Storage unavailable'); } });
          inline.refresh();
          return { open: inline.stats().open, click: fire('#one').prevented };
        });
        assert.deepEqual(result, { open: 0, click: false });
      } finally { await context.close(); }
      const archive = await setup({ archive: true });
      try {
        assert.deepEqual(await archive.page.evaluate(() => ({ click: fire('#one').prevented, open: inline.stats().open })), { click: false, open: 0 });
      } finally { await archive.context.close(); }
    });
  } finally { await browser.close(); }
});
