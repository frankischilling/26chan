import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { nativeCommentText } from '../../apps/public/client/native-filter-html.js';
import { FILTER_LIMITS } from '../../apps/public/client/native-filter-limits.js';
import { parseQuotePreviewSnapshot } from '../../apps/public/client/native-updater-snapshot.js';

const origin = 'https://board.example', mediaOrigin = 'https://media.example';
function post(no, inside, thread = '100') {
  const type = no === thread ? 'op' : 'reply';
  return `<article class="postContainer ${type}Container" id="pc${no}"><div class="post ${type}" id="p${no}"><div class="postInfo" id="pi${no}"><span class="name">Anonymous</span><a class="postNum" href="/demo/thread/${thread}#p${no}">No.${no}</a></div><blockquote class="postMessage" id="m${no}">${inside}</blockquote><details class="postActions"><summary>Delete or report</summary><form method="post" action="/demo/delete"><input type="hidden" name="no" value="${no}"><input type="password" name="password" id="delete${no}" autocomplete="off" required><button>Delete</button></form></details></div></article>`;
}
function envelope(no = '120', inside = 'Remote safe &lt;script&gt; <s>secret</s>') {
  return { version: 1, board: 'demo', thread: '100', post: { no, file_deleted: false, html: post(no, inside) } };
}
const parsed = no => parseQuotePreviewSnapshot(JSON.stringify(envelope(no)), { origin, mediaOrigin, board: 'demo', post: no });

test('isolated DOM quote preview contracts', async t => {
  const bundle = await readFile(new URL('../../apps/public/static/native-filter.v1.js', import.meta.url), 'utf8');
  const css = await readFile(new URL('../../apps/public/static/board.css', import.meta.url), 'utf8')
    + await readFile(new URL('../../apps/public/static/themes/common.css', import.meta.url), 'utf8');
  const browser = await chromium.launch({ headless: true });
  try {
    async function setup({ mobile = false, width = 1000, remote = false, handler } = {}) {
      const context = await browser.newContext({ viewport: { width, height: 700 } });
      const page = await context.newPage();
      const requests = [];
      const fixture = '<!doctype html><html><head><style>' + css + '\n'
        + '#p100{position:absolute;left:25px;top:25px;width:500px;max-width:90vw}'
        + '#p101{position:absolute;left:25px;top:130px;width:500px;max-width:90vw}'
        + '#p110{position:absolute;left:35px;top:350px;width:500px;max-width:90vw}'
        + '</style></head><body><main class="board"><section class="thread" id="t100">'
        + post('100', 'Opening post')
        + post('101', 'Local safe <s>secret</s> https://preview.test/path <a href="https://existing.test/path" rel="nofollow noreferrer noopener">existing</a>')
        + post('110', '<a id="quote" class="quotelink" href="/demo/post/101">&gt;&gt;101</a>')
        + '</section></main></body></html>';
      await page.route('**/*', async route => {
        const url = new URL(route.request().url());
        if (url.origin === origin && url.pathname === '/static/native-filter.v1.js') {
          await route.fulfill({ contentType: 'text/javascript', body: bundle }); return;
        }
        if (url.origin === origin && url.pathname.startsWith('/_watch/')) {
          requests.push({ url: url.href, headers: await route.request().allHeaders(), method: route.request().method() });
          if (handler) await handler(route); else await route.fulfill({ contentType: 'application/json', body: JSON.stringify(envelope()) });
          return;
        }
        if (url.origin === origin && url.pathname === '/demo/thread/100') {
          await route.fulfill({ contentType: 'text/html', body: fixture }); return;
        }
        requests.push({ url: url.href });
        if (url.origin === 'https://tracker.example') await route.fulfill({ contentType: 'text/plain', body: 'healthy fixture control' });
        else await route.abort();
      });
      await page.goto(`${origin}/demo/thread/100`);
      await page.evaluate(async ({ origin, mediaOrigin, mobile, remote }) => {
        const api = await import('/static/native-filter.v1.js');
        window.api = api; window.settings = {}; window.pending = []; window.cancelled = 0;
        window.over = () => document.getElementById('quote').dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
        window.out = () => document.getElementById('quote').dispatchEvent(new MouseEvent('mouseout', { bubbles: true, relatedTarget: document.body }));
        window.mount = () => api.mountNativeQuotePreview({ root: document.querySelector('.board'), board: 'demo', thread: '100', origin, mediaOrigin,
          userAgent: mobile ? 'Test Mobile browser' : 'Windows NT desktop', settings: () => window.settings,
          ...(remote ? {} : { transport: { load(ref, { signal }) {
            return new Promise(resolve => window.pending.push({ ref, signal, resolve }));
          }, cancel() { window.cancelled++; } } }),
          decorate: () => window.linkification?.refresh(),
        });
        window.mounted = window.mount();
      }, { origin, mediaOrigin, mobile, remote });
      return { page, context, requests };
    }

    await t.test('hover alone applies source highlight variants and restores only owned effects', async () => {
      const { page, context } = await setup({ width: 375 });
      try {
        const result = await page.evaluate(() => {
          const link = document.getElementById('quote'), target = document.getElementById('p101');
          link.focus();
          const output = { focus: !!document.getElementById('quote-preview') || target.classList.contains('highlight'),
            narrowDesktopCompanions: document.querySelectorAll('.quoteLink').length };
          window.over(); output.highlight = target.classList.contains('highlight');
          output.visiblePopup = !!document.getElementById('quote-preview'); window.out();
          output.cleaned = !target.classList.contains('highlight');
          target.classList.add('highlight'); window.over();
          output.anti = target.classList.contains('highlight-anti');
          output.antiColor = getComputedStyle(target).backgroundColor;
          window.out(); output.keptExisting = target.classList.contains('highlight') && !target.classList.contains('highlight-anti');
          output.cleanedAttribute = !target.hasAttribute('data-quote-anti'); target.classList.remove('highlight');
          history.replaceState(null, '', '#p101'); window.over(); output.hashAnti = target.classList.contains('highlight-anti'); window.out();
          history.replaceState(null, '', '#p100'); link.setAttribute('href', '#p100'); window.over();
          output.opAnti = document.getElementById('p100').classList.contains('highlight-anti'); window.out();
          history.replaceState(null, '', location.pathname); link.setAttribute('href', '/demo/post/101');
          window.settings = { quotePreview: false }; window.over(); output.optionOff = target.classList.contains('highlight');
          window.settings = { quotePreview: true, disableAll: true }; window.over(); output.disableAll = target.classList.contains('highlight');
          window.settings = {}; window.over(); window.settings = { quotePreview: false };
          document.dispatchEvent(new Event('4chanSettingsSaved')); output.liveDisabled = target.classList.contains('highlight');
          output.requests = window.pending.length;
          return output;
        });
        assert.deepEqual(result, { focus: false, narrowDesktopCompanions: 0, highlight: true, visiblePopup: false, cleaned: true,
          anti: true, antiColor: 'rgb(232, 166, 144)', keptExisting: true, cleanedAttribute: true, hashAnti: true, opAnti: false,
          optionOff: false, disableAll: false, liveDisabled: false, requests: 0 });
      } finally { await context.close(); }
    });

    await t.test('offscreen and hidden local copies remove credentials, IDs, active resources and controls before construction', async () => {
      const { page, context } = await setup();
      try {
        const result = await page.evaluate(() => {
          const source = document.getElementById('quote'), target = document.getElementById('p101'), article = document.getElementById('pc101');
          const originalAnchor = document.querySelector('#m101 a'), password = document.getElementById('delete101');
          password.value = 'FILLED-PASSWORD-MUST-NOT-COPY'; password.setAttribute('value', 'ATTRIBUTE-PASSWORD-MUST-NOT-COPY');
          Object.defineProperty(password, 'value', { get() { throw new Error('read password'); } });
          password.getAttribute = () => { throw new Error('read password attribute'); };
          target.cloneNode = article.cloneNode = () => { throw new Error('unsafe post clone'); };
          const menu = document.createElement('button'); menu.className = 'postMenuBtn'; menu.textContent = 'PRIVATE-MENU'; target.append(menu);
          const image = document.createElement('img'); image.setAttribute('src', 'https://tracker.example/local.png'); image.setAttribute('alt', 'tracker'); target.append(image);
          const video = document.createElement('video'); video.setAttribute('autoplay', ''); video.setAttribute('src', 'https://tracker.example/local.webm'); target.append(video);
          const safe = document.createElement('img'); safe.setAttribute('src', 'https://media.example/demo/1s.jpg'); safe.setAttribute('srcset', 'https://tracker.example/large.png 2x'); safe.setAttribute('alt', 'safe'); target.append(safe);
          target.style.top = '1300px';
          window.settings = { linkify: true };
          window.linkification = window.api.mountNativeLinkification({ root: document.querySelector('.board'), settings: () => window.settings,
            mobile: { matches: false }, readNeverMobile: () => null });
          const create = document.createElement.bind(document), built = [];
          document.createElement = (tag, ...rest) => {
            const element = create(tag, ...rest); built.push(tag); return element;
          };
          window.over(); const popup = document.getElementById('quote-preview');
          const output = { shown: !!popup, text: popup?.textContent, ids: popup?.querySelectorAll('[id]').length,
            controls: popup?.querySelectorAll('form,input,button,details,video,audio,iframe,.postMenuBtn').length,
            images: [...(popup?.querySelectorAll('img') ?? [])].map(img => [img.getAttribute('src'), img.hasAttribute('srcset')]),
            linkified: popup?.querySelectorAll('a[data-native-linkified]').length,
            builtControls: built.filter(tag => ['form', 'input', 'button', 'video', 'audio', 'iframe'].includes(tag)),
            sourceIdentity: source === document.getElementById('quote'),
            originalAnchor: originalAnchor === document.querySelector('#m101 a[href="https://existing.test/path"]'),
            revealed: popup?.classList.contains('reveal-spoilers') };
          window.settings = { linkify: false }; document.dispatchEvent(new Event('4chanSettingsSaved'));
          output.latestLinkification = popup?.querySelectorAll('a[data-native-linkified]').length;
          window.out(); output.cleaned = !document.getElementById('quote-preview');
          target.style.top = '130px'; target.classList.add('post-hidden'); window.over();
          output.hiddenTargetCopy = !!document.getElementById('quote-preview'); window.out();
          target.classList.remove('post-hidden'); target.style.top = '1300px';
          source.parentElement.classList.add('backlink'); window.over();
          output.backlinkRevealed = document.getElementById('quote-preview')?.classList.contains('reveal-spoilers');
          window.out(); document.createElement = create;
          return output;
        });
        assert.equal(result.shown, true); assert.match(result.text, /Local safe/);
        assert.doesNotMatch(result.text, /PASSWORD|PRIVATE-MENU/);
        assert.equal(result.ids, 0); assert.equal(result.controls, 0); assert.deepEqual(result.builtControls, []);
        assert.deepEqual(result.images, [[`${mediaOrigin}/demo/1s.jpg`, false]]);
        assert.equal(result.linkified, 1); assert.equal(result.latestLinkification, 0);
        for (const key of ['sourceIdentity', 'originalAnchor', 'revealed', 'cleaned', 'hiddenTargetCopy']) assert.equal(result[key], true, key);
        assert.equal(result.backlinkRevealed, false);
      } finally { await context.close(); }
    });

    await t.test('mobile companions preserve filter semantics and decline the entire message at HTML or text bounds', async () => {
      const { page, context } = await setup({ mobile: true });
      try {
        const result = await page.evaluate(({ html, field }) => {
          const root = document.querySelector('.board'), source = document.getElementById('quote');
          const small = source.parentElement, original = source;
          const beforeSmall = small.innerHTML;
          function message(body) {
            const element = document.createElement('blockquote'); element.className = 'postMessage'; element.innerHTML = body; root.append(element); return element;
          }
          const anchor = '<a class="quotelink" href="/demo/post/101">&gt;&gt;101</a>';
          const crowded = message(anchor.repeat(700));
          const nbsp = message('\u00a0'.repeat(Math.floor((html - anchor.length - 5) / 6)) + anchor);
          const fullText = message('x'.repeat(field - 5) + anchor);
          const initial = [crowded, nbsp, fullText].map(element => element.innerHTML);
          window.mounted.refresh();
          function click(link, modifiers = {}) {
            let prevented;
            window.addEventListener('click', event => { prevented = event.defaultPrevented; event.preventDefault(); }, { once: true });
            link.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, ...modifiers }));
            return prevented;
          }
          const output = { small: small.innerHTML, identity: original === document.getElementById('quote'),
            companion: [source.nextSibling?.className, source.nextSibling?.textContent, source.nextSibling?.getAttribute('href')],
            bounded: [crowded, nbsp, fullText].map((element, index) => ({ before: initial[index], after: element.innerHTML, helpers: element.querySelectorAll('.quoteLink').length })),
            previewClick: click(source), navigationClick: click(source.nextSibling), modifiedClick: click(source, { ctrlKey: true }),
            boundedClick: click(fullText.querySelector('a')) };
          window.out();
          window.settings = { quotePreview: false }; document.dispatchEvent(new Event('4chanSettingsSaved'));
          output.disabled = document.querySelectorAll('.quoteLink').length;
          output.restored = small.innerHTML === beforeSmall.replace('<a class="quoteLink" href="/demo/post/101"> #</a>', '');
          output.identityAfterDisable = original === document.getElementById('quote');
          window.settings = {}; document.dispatchEvent(new Event('4chanSettingsSaved'));
          output.reenabled = small.querySelectorAll('.quoteLink').length;
          window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
          output.suspended = small.querySelectorAll('.quoteLink').length;
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
          output.resumed = small.querySelectorAll('.quoteLink').length;
          window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: false }));
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
          output.disposed = small.querySelectorAll('.quoteLink').length;
          return output;
        }, { html: FILTER_LIMITS.html, field: FILTER_LIMITS.field });
        assert.equal(nativeCommentText(result.small), '>>101 #');
        assert.deepEqual(result.companion, ['quoteLink', ' #', '/demo/post/101']);
        for (const row of result.bounded) {
          assert.ok(row.before.length <= FILTER_LIMITS.html); assert.equal(row.after, row.before); assert.equal(row.helpers, 0);
          assert.doesNotThrow(() => nativeCommentText(row.after));
        }
        assert.equal(result.previewClick, true); assert.equal(result.navigationClick, false);
        assert.equal(result.modifiedClick, false); assert.equal(result.boundedClick, false);
        for (const key of ['identity', 'restored', 'identityAfterDisable']) assert.equal(result[key], true, key);
        for (const key of ['disabled', 'suspended', 'disposed']) assert.equal(result[key], 0, key);
        assert.equal(result.reenabled, 1); assert.equal(result.resumed, 1);
      } finally { await context.close(); }
    });

    await t.test('late remote results, hidden sources and page suspension cannot reopen a popup', async () => {
      const { page, context } = await setup();
      try {
        await page.evaluate(() => {
          document.getElementById('quote').setAttribute('href', '/demo/post/120'); window.over();
          document.getElementById('p110').classList.add('post-hidden');
        });
        await page.waitForFunction(() => window.pending[0]?.signal.aborted && document.getElementById('quote').style.cursor !== 'wait');
        assert.equal(await page.evaluate(async result => {
          window.pending[0].resolve(result); await new Promise(resolve => setTimeout(resolve, 0));
          return !!document.getElementById('quote-preview');
        }, parsed('120')), false);
        await page.evaluate(() => {
          document.getElementById('p110').classList.remove('post-hidden'); window.over(); window.out();
          document.getElementById('quote').setAttribute('href', '/demo/post/121'); window.over();
        });
        assert.equal(await page.evaluate(async result => {
          window.pending[1].resolve(result); await new Promise(resolve => setTimeout(resolve, 0));
          return !!document.getElementById('quote-preview');
        }, parsed('120')), false);
        await page.evaluate(result => window.pending[2].resolve(result), parsed('121'));
        await page.waitForSelector('#quote-preview');
        assert.match(await page.locator('#quote-preview').textContent(), /No\.121/);
        await page.evaluate(() => document.getElementById('p110').hidden = true);
        await page.waitForFunction(() => !document.getElementById('quote-preview'));
        await page.evaluate(() => {
          document.getElementById('p110').hidden = false; window.over();
          window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
          window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
        });
        assert.equal(await page.evaluate(async result => {
          window.pending[3].resolve(result); await new Promise(resolve => setTimeout(resolve, 0));
          return !!document.getElementById('quote-preview');
        }, parsed('121')), false);
        assert.equal(await page.evaluate(() => window.pending.every(row => row.signal.aborted)), true);
      } finally { await context.close(); }
    });

    await t.test('real storage events cancel previews and restore mobile navigation across tabs', async () => {
      const { page, context } = await setup({ mobile: true });
      try {
        await page.evaluate(({ origin, mediaOrigin }) => {
          window.mounted.disconnect();
          window.mounted = window.api.mountNativeQuotePreview({ root: document.querySelector('.board'), board: 'demo', thread: '100', origin, mediaOrigin,
            userAgent: 'Mobile', settings: () => JSON.parse(localStorage.getItem('4chan-settings') || '{}'),
            transport: { load: () => new Promise(() => {}), cancel() {} } });
          document.getElementById('p101').style.top = '1300px'; window.over();
        }, { origin, mediaOrigin });
        await page.waitForSelector('#quote-preview');
        const other = await context.newPage();
        await other.route('**/*', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Storage fixture</title>' }));
        await other.goto(`${origin}/storage-fixture`);
        await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ quotePreview: false })));
        await page.waitForFunction(() => !document.getElementById('quote-preview') && !document.querySelector('.quoteLink'));
        await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ quotePreview: true, disableAll: true })));
        await page.evaluate(() => window.over()); assert.equal(await page.locator('#quote-preview').count(), 0);
        await other.evaluate(() => localStorage.removeItem('4chan-settings'));
        await page.waitForSelector('.quoteLink');
        await page.evaluate(() => window.over()); await page.waitForSelector('#quote-preview');
      } finally { await context.close(); }
    });

    await t.test('release worker validates remote recipes before DOM loads and keeps errors observable without replacing links', async () => {
      let mode = 'ok';
      const { page, context, requests } = await setup({ remote: true, handler: async route => {
        if (mode === 'missing') { await route.fulfill({ status: 404, body: '' }); return; }
        const value = mode === 'hostile' ? envelope('120', '<img src="https://tracker.example/rejected.png" alt="bad">') : envelope();
        await route.fulfill({ contentType: mode === 'mime' ? 'text/html' : 'application/json', body: JSON.stringify(value) });
      } });
      try {
        await context.addCookies([{ name: 'fixture-private', value: 'fixture-only', url: origin, httpOnly: true }]);
        await page.evaluate(async () => {
          window.workerCounts = { created: 0, terminated: 0 };
          const OriginalWorker = window.Worker;
          window.Worker = class extends OriginalWorker {
            constructor(...args) { super(...args); window.workerCounts.created++; }
            terminate() { window.workerCounts.terminated++; super.terminate(); }
          };
          window.originalQuote = document.getElementById('quote');
          window.originalQuote.setAttribute('href', '/demo/post/120');
          await fetch('https://tracker.example/healthy-control');
        });
        for (const state of ['ok', 'hostile', 'mime', 'missing', 'ok']) {
          mode = state;
          await page.evaluate(() => {
            window.mounted.disconnect(); window.mounted = window.mount();
            window.over();
            if (document.getElementById('quote').style.cursor !== 'wait') throw new Error('Request did not start');
          });
          await page.waitForFunction(() => document.getElementById('quote').style.cursor !== 'wait');
          assert.equal(await page.locator('#quote-preview').count(), state === 'ok' ? 1 : 0, state);
          assert.equal(await page.evaluate(() => document.getElementById('quote').classList.contains('deadlink')), state === 'missing');
          assert.equal(await page.evaluate(() => window.originalQuote === document.getElementById('quote')), true);
          if (state === 'ok') {
            assert.match(await page.locator('#quote-preview').textContent(), /Remote safe <script>/);
            assert.equal(await page.locator('#quote-preview form,#quote-preview input,#quote-preview [id]').count(), 0);
          }
          await page.evaluate(() => window.out());
          assert.equal(await page.evaluate(() => document.getElementById('quote').classList.contains('deadlink')), false);
        }
        assert.deepEqual(await page.evaluate(() => window.workerCounts), { created: 3, terminated: 3 });
        const apiRequests = requests.filter(row => row.url.startsWith(`${origin}/_watch/`));
        assert.equal(apiRequests.length, 5);
        for (const request of apiRequests) {
          assert.equal(request.url, `${origin}/_watch/demo/post/120`); assert.equal(request.method, 'GET');
          assert.equal(request.headers.cookie, undefined); assert.equal(request.headers.authorization, undefined);
          assert.equal(request.headers['if-none-match'], undefined);
        }
        assert.deepEqual(requests.filter(row => row.url.startsWith('https://tracker.example')).map(row => row.url), ['https://tracker.example/healthy-control']);
      } finally { await context.close(); }
    });
  } finally { await browser.close(); }
});
