import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { nativeCommentText } from '../../apps/public/client/native-filter-html.js';

const origin = 'https://backlinks.example';
const escaped = text => text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
function quote(no, href = `/demo/post/${no}`, text = `>>${no}`) {
  return `<a class="quotelink" href="${escaped(href)}">${escaped(text)}</a>`;
}
function post(no, content = '', thread = '100') {
  const kind = no === thread ? 'op' : 'reply';
  return `<article class="postContainer ${kind}Container" id="pc${no}"><div class="post ${kind}" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span><a class="postNum" href="/demo/thread/${thread}#p${no}">No.${no}</a></div><blockquote class="postMessage" id="m${no}">${content}</blockquote></div></article>`;
}
const section = (thread, posts) => `<section class="thread" id="t${thread}">${posts}</section>`;

test('isolated backlink DOM and shared callback contracts', async t => {
  const files = {};
  for (const name of ['native-filter.v1.js', 'native-backlinks.v1.js']) {
    files[`/static/${name}`] = await readFile(new URL(`../../apps/public/static/${name}`, import.meta.url), 'utf8');
  }
  const css = await readFile(new URL('../../apps/public/static/board.css', import.meta.url), 'utf8')
    + await readFile(new URL('../../apps/public/static/themes/common.css', import.meta.url), 'utf8');
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup(html, { thread = '100', width = 1000, ua = 'Desktop', config = {}, storage = false } = {}) {
      const context = await browser.newContext({ viewport: { width, height: 700 } });
      const page = await context.newPage(), requests = [];
      const fixture = `<!doctype html><html><head><style>${css}</style></head><body><main class="board">${html}</main></body></html>`;
      await context.route('**/*', async route => {
        const url = new URL(route.request().url());
        if (url.origin === origin && files[url.pathname]) {
          await route.fulfill({ contentType: 'text/javascript; charset=utf-8', body: files[url.pathname] });
        } else if (url.origin === origin && ['/demo/', '/demo/thread/100'].includes(url.pathname)) {
          await route.fulfill({ contentType: 'text/html', body: fixture });
        } else if (url.origin === origin && ['/favicon.ico', '/static/themes/fade.png', '/static/themes/fade-blue.png'].includes(url.pathname)) {
          await route.fulfill({ status: 204, body: '' });
        } else { requests.push(url.href); await route.abort(); }
      });
      await page.goto(`${origin}${thread ? '/demo/thread/100' : '/demo/'}`);
      await page.evaluate(({ thread, ua, config, storage }) => {
        window.mountTask = (async () => {
        window.setupStage = 'filter import';
        window.api = await import('/static/native-filter.v1.js');
        window.setupStage = 'backlink import';
        window.pageApi = await import('/static/native-backlinks.v1.js');
        window.setupStage = 'mount';
        window.config = config; window.tracked = new Set(); window.remoteCalls = 0;
        window.settings = () => storage ? JSON.parse(localStorage.getItem('4chan-settings') || '{}') : window.config;
        window.root = document.querySelector('.board');
        window.mobile = matchMedia('(max-width: 480px)');
        window.options = { root: window.root, board: 'demo', thread, origin: location.origin,
          settings: window.settings, mobile: window.mobile,
          readNeverMobile: () => localStorage.getItem('4chan_never_show_mobile'), quoteTarget: window.api.quoteTarget,
          changed: () => window.preview?.refresh() };
        window.backlinks = window.pageApi.mountNativeBacklinks(window.options);
        window.setupStage = 'preview mount';
        window.preview = window.api.mountNativeQuotePreview({ root: window.root, board: 'demo', thread,
          origin: location.origin, settings: window.settings, userAgent: ua,
          companion: link => window.backlinks.companion(link),
          decoratePreview: (...args) => window.backlinks.decoratePreview(...args),
          transport: { cancel() {}, async load() { window.remoteCalls++; return { status: 'invalid-preview' }; } },
        });
        window.rows = no => Array.from(document.querySelectorAll(`#bl_${no} > span > a.quotelink`), link => link.textContent);
        window.refresh = () => { window.backlinks.refresh(); window.preview.refresh(); };
        window.track = ids => {
          window.tracked = new Set(ids);
          window.api.markNativeTrackedQuotes(window.root, window.tracked, window.settings().disableAll !== true, window.backlinks);
          window.refresh();
        };
        window.over = link => link.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
        window.out = link => link.dispatchEvent(new MouseEvent('mouseout', { bubbles: true, relatedTarget: document.body }));
        window.flush = () => new Promise(resolve => setTimeout(resolve, 0));
        window.setupStage = 'ready';
        if (!window.backlinks || !window.preview) throw new Error('Fixture controllers did not mount');
        })().catch(error => { window.setupError = error.message; });
        return true;
      }, { thread, ua, config, storage });
      await page.waitForFunction(() => window.setupStage === 'ready' || window.setupError).catch(async error => {
        const stage = await page.evaluate(() => window.setupStage);
        await context.close();
        throw new Error(`${error.message}; setup stage: ${stage}; unexpected requests: ${JSON.stringify(requests)}`);
      });
      assert.equal(await page.evaluate(() => window.setupError), undefined);
      return { context, page, requests };
    }

    await t.test('source order, self-quotes, strict targets and first-discovery membership survive repeated refresh', async () => {
      const html = section('300', post('300', quote('100'), '300')
        + post('302', quote('100') + quote('100') + quote('302') + quote('999')
          + quote('100', '/other/post/100', '>>>/other/100') + quote('101', '/demo/thread/1#p101'), '300'))
        + section('100', post('100', quote('100')) + post('101', quote('100')));
      const { page, context, requests } = await setup(html, { thread: null });
      try {
        const first = await page.evaluate(() => {
          window.first = document.querySelector('#m302 .quotelink'); window.row = document.querySelector('#bl_100 .quotelink');
          window.refresh(); window.refresh();
          return { order: window.rows('100'), self: window.rows('302'), wrongThread: window.rows('101'),
            labels: Array.from(document.querySelectorAll('#m302 .quotelink'), link => link.textContent),
            op: document.querySelector('#m100 .quotelink').textContent,
            hrefs: Array.from(document.querySelectorAll('#bl_100 .quotelink'), link => link.getAttribute('href')) };
        });
        assert.deepEqual(first.order, ['>>300', '>>302', '>>100', '>>101']);
        assert.deepEqual(first.self, ['>>302']); assert.deepEqual(first.wrongThread, []);
        assert.deepEqual(first.labels, ['>>100', '>>100', '>>302', '>>999', '>>>/other/100', '>>101']);
        assert.equal(first.op, '>>100 (OP)');
        assert.deepEqual(first.hrefs, ['/demo/thread/300#p300', '/demo/thread/300#p302', '/demo/thread/100#p100', '/demo/thread/100#p101']);
        await page.evaluate(html => {
          const holder = document.createElement('div'); holder.innerHTML = html;
          window.root.append(...holder.childNodes); window.refresh();
          window.config.backlinks = false; window.refresh(); window.config.backlinks = true; window.refresh();
        }, section('999', post('999', 'Later target', '999') + post('1000', quote('100'), '999')));
        assert.deepEqual(await page.evaluate(() => ({ oldMissing: window.rows('999'), order: window.rows('100'),
          sameSource: window.first === document.querySelector('#m302 .quotelink') })),
        { oldMissing: [], order: ['>>300', '>>302', '>>100', '>>101', '>>1000'], sameSource: true });
        await page.evaluate(() => { document.getElementById('pc302').remove(); window.refresh(); });
        assert.deepEqual(await page.evaluate(() => window.rows('100')), ['>>300', '>>100', '>>101', '>>1000']);
        assert.deepEqual(requests, []);
      } finally { await context.close(); }
    });

    await t.test('large IDs remain distinct and missing arrows do not modify explicit cross-board labels', async () => {
      const base = '9007199254740992', next = '9007199254740993', max = '9223372036854775807';
      const { page, context } = await setup(section(base, post(base, 'Opening', base)
        + post(next, quote(base) + quote(max) + quote('123', '/demo/post/123', '>>>/demo/123')
          + quote(base, `/foreign/post/${base}`, `>>>/foreign/${base}`), base)), { thread: base });
      try {
        assert.deepEqual(await page.evaluate(() => ({ labels: Array.from(document.querySelectorAll('.postMessage a'), a => a.textContent),
          rows: window.rows('9007199254740992') })),
        { labels: [`>>${base} (OP)`, `>>${max} →`, '>>>/demo/123', `>>>/foreign/${base}`], rows: [`>>${next}`] });
      } finally { await context.close(); }
    });

    await t.test('owned labels preserve You order and filter projection removes only the added suffix nodes', async () => {
      const { page, context } = await setup(section('100', post('100', 'Opening')
        + post('101', quote('100') + ' ' + quote('999') + '<s>literal &amp; &nbsp; text</s>')
        + post('102', 'Literal (OP) ' + quote('100', '/demo/post/100', '>>100 (OP)'))));
      try {
        const results = await page.evaluate(async () => {
          window.track(['100']); window.track(['100']);
          const message = document.getElementById('m101');
          const output = { label: message.querySelector('a').textContent, projection: window.backlinks.commentHTML(message),
            literal: window.backlinks.commentHTML(document.getElementById('m102')), identity: message.querySelector('a') };
          window.config.filter = true;
          window.filterRaw = JSON.stringify([{ type: 2, pattern: '/OP|→/', boards: '', active: true, hide: false, color: '#ff0000' }]);
          const matcher = new window.api.NativeFilterMatcher();
          window.filters = window.api.mountNativeFilters({ board: 'demo', threadId: '100', settings: window.settings,
            read: () => window.filterRaw, save() {}, changed() {}, getTracked: () => new Set(),
            commentHTML: window.backlinks.commentHTML, match: (...args) => matcher.match(...args) });
          output.settled = await window.filters.refreshSettled();
          output.nativeOnlyMatched = document.getElementById('p101').classList.contains('filter-hl');
          output.literalMatched = document.getElementById('p102').classList.contains('filter-hl');
          window.filterRaw = JSON.stringify([{ type: 2, pattern: '/You/', boards: '', active: true, hide: false, color: '#ff0000' }]);
          output.youSettled = await window.filters.refreshSettled();
          output.youMatched = document.getElementById('p101').classList.contains('filter-hl');
          window.config.backlinks = false; window.refresh();
          output.disabled = message.querySelector('a').textContent;
          output.sameAnchor = output.identity === message.querySelector('a'); delete output.identity;
          window.config.backlinks = true; window.track([]); window.refresh();
          output.untracked = message.querySelector('a').textContent;
          output.rows = window.rows('100');
          return output;
        });
        assert.equal(results.label, '>>100 (You) (OP)');
        assert.equal(nativeCommentText(results.projection), '>>100 (You) >>999literal & \u00a0 text');
        assert.equal(nativeCommentText(results.literal), 'Literal (OP) >>100 (OP)');
        assert.equal(results.settled, true); assert.equal(results.youSettled, true);
        assert.equal(results.nativeOnlyMatched, false); assert.equal(results.literalMatched, true); assert.equal(results.youMatched, true);
        assert.equal(results.disabled, '>>100 (You)'); assert.equal(results.untracked, '>>100 (OP)');
        assert.equal(results.sameAnchor, true); assert.deepEqual(results.rows, ['>>101', '>>102']);
      } finally { await context.close(); }
    });

    await t.test('mobile row ownership is separate from preview UA and restores correctly across layout changes', async () => {
      for (const ua of ['Desktop', 'Mobile test']) {
        const { page, context } = await setup(section('100', post('100', 'Opening') + post('101', 'Target')
          + post('102', quote('101'))), { width: 375, ua, config: { quotePreview: false } });
        try {
          const initial = await page.evaluate(() => {
            window.link = document.querySelector('#bl_101 .quotelink'); window.hash = window.link.nextSibling;
            return { parent: document.getElementById('bl_101').parentElement.id, count: document.querySelectorAll('#bl_101 .quoteLink').length,
              owned: window.backlinks.companion(window.link) === window.hash };
          });
          assert.deepEqual(initial, { parent: 'p101', count: 1, owned: true });
          await page.evaluate(async () => { window.config.quotePreview = true; window.refresh(); await window.flush(); window.refresh(); });
          assert.equal(await page.locator('#bl_101 .quoteLink').count(), 1);
          assert.equal(await page.evaluate(() => window.link === document.querySelector('#bl_101 .quotelink')), true);
          await page.evaluate(() => { window.config.quotePreview = false; window.refresh(); });
          assert.equal(await page.evaluate(() => window.hash === document.querySelector('#bl_101 .quoteLink')), true);
          await page.setViewportSize({ width: 1000, height: 700 });
          await page.waitForFunction(() => document.getElementById('bl_101').parentElement.id === 'pi101');
          assert.equal(await page.locator('#bl_101 .quoteLink').count(), 0);
          await page.evaluate(async () => { window.config.quotePreview = true; window.refresh(); await window.flush(); });
          assert.equal(await page.locator('#bl_101 .quoteLink').count(), ua.startsWith('Mobile') ? 1 : 0);
          await page.setViewportSize({ width: 375, height: 700 });
          await page.waitForFunction(() => document.getElementById('bl_101').parentElement.id === 'p101');
          assert.equal(await page.locator('#bl_101 .quoteLink').count(), 1);
          await page.evaluate(() => { localStorage.setItem('4chan_never_show_mobile', 'true'); window.refresh(); });
          assert.equal(await page.locator('#bl_101').evaluate(node => node.parentElement.id), 'pi101');
        } finally { await context.close(); }
      }
    });

    await t.test('hidden sources still contribute while target hiding controls the row and preview lifecycle', async () => {
      const { page, context } = await setup(section('100', post('100', 'Opening') + post('101', 'Target') + post('102', quote('101'))));
      try {
        const output = await page.evaluate(async () => {
          document.getElementById('pc102').classList.add('post-hidden'); window.refresh();
          const link = document.querySelector('#bl_101 .quotelink');
          window.over(link);
          const shown = !!document.getElementById('quote-preview');
          document.getElementById('p101').classList.add('post-hidden'); window.refresh();
          const result = { rows: window.rows('101'), shown, hidden: getComputedStyle(document.getElementById('bl_101')).display,
            cancelled: !document.getElementById('quote-preview') };
          document.getElementById('p101').classList.remove('post-hidden'); window.refresh();
          result.restored = getComputedStyle(document.getElementById('bl_101')).display !== 'none';
          return result;
        });
        assert.deepEqual(output, { rows: ['>>102'], shown: true, hidden: 'none', cancelled: true, restored: true });
      } finally { await context.close(); }
    });

    await t.test('local previews reconstruct known rows without IDs or clones and apply the exact dotted cue', async () => {
      const { page, context } = await setup(section('100', post('100', 'Opening') + post('101', 'Target') + post('102', 'Second target')
        + post('110', quote('101') + quote('102') + '<s>secret</s>') + post('120', quote('110'))));
      try {
        const result = await page.evaluate(() => {
          document.getElementById('p110').style.position = 'absolute'; document.getElementById('p110').style.top = '1000px';
          const originalClone = Node.prototype.cloneNode;
          Node.prototype.cloneNode = () => { throw new Error('preview cloned a live node'); };
          try {
            const source = document.querySelector('#bl_101 .quotelink'); window.over(source);
            const popup = document.getElementById('quote-preview');
            const output = { shown: !!popup, rows: Array.from(popup?.querySelectorAll('.backlink .quotelink') || [], a => a.textContent),
              dotted: popup?.querySelector('.dotted')?.textContent, ids: popup?.querySelectorAll('[id]').length,
              spoilers: popup?.classList.contains('reveal-spoilers'), originalRows: window.rows('110') };
            window.out(source);
            document.getElementById('bl_101').removeAttribute('id'); window.over(source);
            output.noIdDot = !!document.querySelector('#quote-preview .dotted'); window.out(source);
            document.querySelector('#pi101 > .backlink').id = 'bl_101';
            window.track(['101']); window.over(source);
            output.trackedDot = !!document.querySelector('#quote-preview .dotted'); window.out(source);
            const original = document.querySelector('#m120 .quotelink'); window.over(original);
            output.forwardSpoilers = document.getElementById('quote-preview')?.classList.contains('reveal-spoilers'); window.out(original);
            return output;
          } finally { Node.prototype.cloneNode = originalClone; }
        });
        assert.deepEqual(result, { shown: true, rows: ['>>120'], dotted: '>>101', ids: 0, spoilers: false,
          originalRows: ['>>120'], noIdDot: false, trackedDot: false, forwardSpoilers: true });
      } finally { await context.close(); }
    });

    await t.test('annotation and graph bounds decline entire sources with healthy controls', async () => {
      const { page, context } = await setup(section('100', post('100', 'Opening')
        + post('101', quote('100').repeat(512)) + post('102', quote('100').repeat(513))
        + post('103', 'x'.repeat(16374) + quote('100')) + post('104', 'x'.repeat(16375) + quote('100'))
        + post('105', '<span>'.repeat(33) + quote('100') + '</span>'.repeat(33))));
      try {
        assert.deepEqual(await page.evaluate(() => ({ rows: window.rows('100'),
          count: Array.from(document.querySelectorAll('#m101 a'), a => a.textContent).filter(t => t.endsWith(' (OP)')).length,
          over: document.querySelector('#m102 a').textContent, textOver: document.querySelector('#m104 a').textContent,
          deep: document.querySelector('#m105 a').textContent })),
        { rows: ['>>101', '>>103'], count: 512, over: '>>100', textOver: '>>100', deep: '>>100' });
      } finally { await context.close(); }
      const targets = Array.from({ length: 512 }, (_, i) => String(i + 101));
      const allQuotes = targets.map(no => quote(no)).join('');
      const large = await setup(section('100', post('100', 'Opening') + targets.map(no => post(no, 'Target')).join('')
        + Array.from({ length: 9 }, (_, i) => post(String(1000 + i), allQuotes)).join('')));
      try {
        assert.deepEqual(await large.page.evaluate(() => ({ rows: document.querySelectorAll('.backlink > span').length,
          first: window.rows('101'), last: window.rows('612') })),
        { rows: 4096, first: Array.from({ length: 8 }, (_, i) => `>>${1000 + i}`), last: Array.from({ length: 8 }, (_, i) => `>>${1000 + i}`) });
      } finally { await large.context.close(); }
    });

    await t.test('popup row bounds omit the entire extra projection and never reuse original IDs', async () => {
      const html = section('100', post('100', 'Opening') + post('101', 'Target')
        + Array.from({ length: 129 }, (_, i) => post(String(200 + i), quote('101'))).join(''));
      const { page, context } = await setup(html);
      try {
        const output = await page.evaluate(() => {
          const popup = document.createElement('div'); popup.innerHTML = '<div class="postInfo"></div><blockquote class="postMessage">Target</blockquote>';
          const post = document.getElementById('p101');
          window.backlinks.decoratePreview(popup, post, null, { nodes: 16384, characters: 262144 });
          const large = popup.querySelectorAll('.backlink').length;
          document.getElementById('pc328').remove(); window.refresh();
          window.backlinks.decoratePreview(popup, post, null, { nodes: 16384, characters: 262144 });
          const healthy = popup.querySelectorAll('.backlink > span').length;
          const ids = popup.querySelectorAll('[id]').length;
          window.backlinks.decoratePreview(popup, post, null, { nodes: 512, characters: 262144 });
          const nodes = popup.querySelectorAll('.backlink').length;
          window.backlinks.decoratePreview(popup, post, null, { nodes: 16384, characters: 100 });
          return { large, healthy, ids, nodes, chars: popup.querySelectorAll('.backlink').length };
        });
        assert.deepEqual(output, { large: 0, healthy: 128, ids: 0, nodes: 0, chars: 0 });
      } finally { await context.close(); }
    });

    await t.test('HTML entity budgets include the final annotations and filter projection preserves browser serialization', async () => {
      const { page, context } = await setup(section('100', post('100', 'Opening')
        + post('101', '<span title="A &lt; B &gt; C &quot;D&quot; &amp; E">literal &nbsp; &amp; &lt;OP&gt;</span><br><pre class="prettyprint">code<wbr>word</pre>' + quote('100'))));
      try {
        const result = await page.evaluate(template => {
          const original = document.getElementById('m101');
          window.config.backlinks = false; window.refresh();
          const before = original.innerHTML;
          window.config.backlinks = true; window.refresh();
          const projected = window.backlinks.commentHTML(original);
          const controls = [];
          for (const extra of [0, 1]) {
            const holder = document.createElement('div'); holder.innerHTML = template.replaceAll('POSTNO', String(201 + extra));
            const article = holder.firstElementChild, message = article.querySelector('.postMessage');
            const span = document.createElement('span'); span.textContent = '\u00a0'.repeat(5000); span.setAttribute('title', '');
            message.prepend(span);
            span.setAttribute('title', 'x'.repeat(window.pageApi.BACKLINK_LIMITS.html - 5 + extra - message.innerHTML.length));
            controls.push({ message, before: message.innerHTML });
            document.getElementById('t100').append(article);
          }
          window.refresh();
          return { before, projected, rows: window.rows('100'), controls: controls.map(({ message, before }) => ({
            beforeLength: before.length, afterLength: message.innerHTML.length,
            label: message.querySelector('a').textContent,
            projectionPreserved: window.backlinks.commentHTML(message) === before,
          })) };
        }, post('POSTNO', quote('100')));
        assert.equal(result.projected, result.before);
        assert.equal(nativeCommentText(result.projected), nativeCommentText(result.before));
        assert.deepEqual(result.rows, ['>>101', '>>201']);
        assert.deepEqual(result.controls, [
          { beforeLength: 65531, afterLength: 65536, label: '>>100 (OP)', projectionPreserved: true },
          { beforeLength: 65532, afterLength: 65532, label: '>>100', projectionPreserved: true },
        ]);
      } finally { await context.close(); }
    });

    await t.test('the total reference bound keeps earlier complete sources and final page exit retires callbacks', async () => {
      const inside = quote('100').repeat(512);
      const { page, context } = await setup(section('100', post('100', 'Opening')
        + Array.from({ length: 33 }, (_, i) => post(String(1000 + i), inside)).join('')));
      try {
        const result = await page.evaluate(template => {
          const initial = window.rows('100');
          const overflow = document.querySelector('#m1032 a').textContent;
          window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: false }));
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
          const holder = document.createElement('div'); holder.innerHTML = template;
          document.getElementById('t100').append(holder.firstElementChild);
          window.refresh();
          return { initial, overflow, retiredRows: window.rows('100'), newLabel: document.querySelector('#m2000 a').textContent };
        }, post('2000', quote('100')));
        assert.deepEqual(result, { initial: Array.from({ length: 32 }, (_, i) => `>>${1000 + i}`),
          overflow: '>>100', retiredRows: [], newLabel: '>>100' });
      } finally { await context.close(); }
    });

    await t.test('the dotted cue charges its complete class suffix and uses the actual owning container ID', async () => {
      const { page, context } = await setup(section('100', post('100', 'Opening') + post('101', 'Target') + post('102', 'Other')
        + post('110', quote('102') + quote('101'))));
      try {
        const result = await page.evaluate(() => {
          const popup = document.createElement('div');
          popup.innerHTML = '<div class="postInfo"></div><blockquote class="postMessage"><a class="quotelink" href="/demo/post/102">&gt;&gt;102</a><a class="quotelink" href="/demo/post/101">&gt;&gt;101</a></blockquote>';
          const post = document.getElementById('p110'), source = document.querySelector('#bl_101 .quotelink');
          window.backlinks.decoratePreview(popup, post, source, { nodes: 0, characters: 6 });
          const six = popup.querySelectorAll('.dotted').length;
          window.backlinks.decoratePreview(popup, post, source, { nodes: 0, characters: 7 });
          const seven = popup.querySelector('.dotted')?.textContent;
          source.parentElement.parentElement.removeAttribute('id');
          window.backlinks.decoratePreview(popup, post, source, { nodes: 0, characters: 7 });
          return { six, seven, missingId: popup.querySelectorAll('.dotted').length };
        });
        assert.deepEqual(result, { six: 0, seven: '>>101', missingId: 0 });
      } finally { await context.close(); }
    });

    await t.test('backlink colors follow pinned quote colors with only the blue-theme override', async () => {
      const { page, context } = await setup(section('100', post('100', 'Opening') + post('101', 'Target') + post('110', quote('101'))));
      try {
        const values = [];
        for (const family of ['futaba', 'burichan', 'tomorrow', 'photon']) {
          await page.evaluate(family => { document.documentElement.style.setProperty('--watcher-icon-family', family); window.refresh(); }, family);
          await page.mouse.move(0, 0);
          const color = await page.locator('#bl_101 .quotelink').evaluate(a => getComputedStyle(a).color);
          await page.locator('#bl_101 .quotelink').hover();
          const hover = await page.locator('#bl_101 .quotelink').evaluate(a => getComputedStyle(a).color);
          values.push([family, color, hover]);
        }
        assert.deepEqual(values, [['futaba', 'rgb(0, 0, 128)', 'rgb(255, 0, 0)'], ['burichan', 'rgb(52, 52, 92)', 'rgb(221, 0, 0)'],
          ['tomorrow', 'rgb(95, 137, 172)', 'rgb(129, 162, 190)'], ['photon', 'rgb(255, 102, 0)', 'rgb(255, 51, 0)']]);
      } finally { await context.close(); }
    });

    await t.test('real storage events and synthetic BFCache lifecycle restore only owned rows and annotations', async () => {
      const { page, context } = await setup(section('100', post('100', 'Opening')
        + post('101', quote('100') + ' original (OP)')), { width: 375, storage: true });
      try {
        const other = await context.newPage(); await other.goto(`${origin}/demo/`);
        await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ backlinks: false })));
        await page.waitForFunction(() => !document.getElementById('bl_100'));
        assert.equal(await page.locator('#m101').textContent(), '>>100 original (OP)');
        await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ backlinks: true })));
        await page.waitForFunction(() => document.getElementById('bl_100'));
        assert.equal(await page.locator('#m101').textContent(), '>>100 (OP) original (OP)');
        await other.evaluate(() => localStorage.setItem('4chan_never_show_mobile', 'true'));
        await page.waitForFunction(() => document.getElementById('bl_100').parentElement.id === 'pi100');
        await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ backlinks: true, disableAll: true })));
        await page.waitForFunction(() => !document.getElementById('bl_100'));
        await other.evaluate(() => localStorage.removeItem('4chan-settings'));
        await page.waitForFunction(() => document.getElementById('bl_100'));
        const result = await page.evaluate(() => {
          const link = document.querySelector('#m101 a');
          window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
          const hidden = document.querySelectorAll('.backlink').length;
          const label = link.textContent;
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true })); window.refresh();
          const restored = window.rows('100');
          window.refresh(); window.refresh();
          const unrelated = document.createElement('div'); unrelated.id = 'unrelated'; unrelated.className = 'backlink'; document.body.append(unrelated);
          window.backlinks.disconnect();
          return { hidden, label, restored, final: link.textContent, original: link === document.querySelector('#m101 a'),
            keptUnrelated: unrelated.isConnected, rows: document.querySelectorAll('.board .backlink').length };
        });
        assert.deepEqual(result, { hidden: 0, label: '>>100', restored: ['>>101'], final: '>>100', original: true, keptUnrelated: true, rows: 0 });
      } finally { await context.close(); }
    });
  } finally { await browser.close(); }
});
