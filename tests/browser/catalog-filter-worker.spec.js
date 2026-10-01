
import { test, expect } from '@playwright/test';
const workerPath = '/static/catalog-filter-core.v1.js';

test('production catalog CSP admits only the fixed filter worker and its bounded jobs keep the page responsive', async ({ page }) => {
  const pageErrors = []; page.on('pageerror', error => pageErrors.push(error.message));
  const response = await page.goto('/test/catalog');
  const origin = new URL(page.url()).origin;
  expect(response.headers()['content-security-policy']).toContain('worker-src '+origin+'/static/native-filter.v1.js '+origin+workerPath+';');
  const outcome = await page.evaluate(async path => {
    const { CatalogFilterMatcher } = await import(path);
    let created = 0, terminated = 0, ticks = 0;
    const createWorker = () => {
      const worker = new Worker(path, { type: 'module' }); created++;
      const stop = worker.terminate.bind(worker); worker.terminate = () => { terminated++; stop(); }; return worker;
    };
    const matcher = new CatalogFilterMatcher({ createWorker });
    const rule = { active: 1, pattern: 'paper', boards: '', hidden: 1, top: 0 };
    const matched = await matcher.match([rule], 'test', [
      { id: '9007199254740992', text: 'PAPER' }, { id: '9007199254740993', text: 'other' },
      { id: '9223372036854775807', text: 'paper' },
    ]);
    const timer = setInterval(() => ticks++, 10);
    const slow = { ...rule, pattern: '/^(a+)+$/' };
    const cards = [{ id: '1', text: 'a'.repeat(50000) + '!' }];
    const timed = await matcher.match([slow], 'test', cards); clearInterval(timer);
    const controller = new AbortController();
    const pending = matcher.match([slow], 'test', cards, { signal: controller.signal }); controller.abort();
    const cancelled = await pending;
    const healthy = await matcher.match([rule], 'test', [{ id: '1', text: 'paper' }]);
    return { matched, timed, cancelled, healthy, ticks, created, terminated };
  }, workerPath);
  expect(outcome.matched).toEqual({ status: 'ok', matches: [{ id: '9007199254740992', filter: 0 }, { id: '9223372036854775807', filter: 0 }] });
  expect(outcome.timed.status).toBe('timeout'); expect(outcome.cancelled.status).toBe('cancelled');
  expect(outcome.healthy).toEqual({ status: 'ok', matches: [{ id: '1', filter: 0 }] });
  expect(outcome.ticks).toBeGreaterThan(0); expect(outcome.created).toBe(4); expect(outcome.terminated).toBe(4);
  expect(pageErrors).toEqual([]);
});

test('catalog worker response CSP denies network, imports and nested workers with healthy browser controls', async ({ page, context, request }) => {
  const pageErrors = []; page.on('pageerror', error => pageErrors.push(error.message));
  await context.route('**/catalog-filter-control', route => route.fulfill({
    contentType: 'text/html', body: '<!doctype html><p>Owned worker control</p>',
  }));
  await context.route('**/catalog-filter-healthy-worker.js', route => route.fulfill({
    contentType: 'text/javascript', body: 'self.onmessage = () => self.postMessage("healthy");',
  }));
  await page.goto('/catalog-filter-control');
  const healthy = await page.evaluate(async () => {
    const worker = new Worker('/catalog-filter-healthy-worker.js', { type: 'module' });
    const response = new Promise((resolve, reject) => {
      worker.onmessage = event => resolve(event.data);
      worker.onerror = () => reject(new Error('Owned control worker failed'));
    });
    worker.postMessage('start');
    try {
      return { worker: await response, ready: (await fetch('/readyz')).status };
    } finally { worker.terminate(); }
  });
  expect(healthy).toEqual({ worker: 'healthy', ready: 200 });
  const release = await request.get(workerPath);
  expect(release.status()).toBe(200);
  const csp = release.headers()['content-security-policy'];
  for (const directive of ['default-src', 'script-src', 'connect-src', 'worker-src']) {
    expect(csp).toContain(`${directive} 'none';`);
  }
  // Keep the release module and response headers. Install the owned probe before
  // its worker listener so the probe's message cannot reach the job evaluator.
  await context.route(`**${workerPath}`, async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `
      if (typeof WorkerGlobalScope !== 'undefined' && globalThis instanceof WorkerGlobalScope) {
      const violations = [];
      self.addEventListener('securitypolicyviolation', event => violations.push({
        directive: event.effectiveDirective, uri: event.blockedURI
      }));
      self.addEventListener('message', async event => {
        event.stopImmediatePropagation();
        const network = await fetch('/readyz').then(() => 'allowed', () => 'blocked');
        const imported = await import('/catalog-filter-healthy-worker.js').then(() => 'allowed', () => 'blocked');
        const nested = await new Promise(resolve => {
          let child;
          let settled = false;
          const finish = status => {
            if (settled) return;
            settled = true;
            clearTimeout(timer);
            if (child) { child.onmessage = null; child.onerror = null; child.terminate(); }
            resolve(status);
          };
          const timer = setTimeout(() => finish('timeout'), 2000);
          try {
            child = new Worker('/catalog-filter-healthy-worker.js', { type: 'module' });
            child.onmessage = event => finish(event.data === 'healthy' ? 'allowed' : 'unexpected-message');
            child.onerror = event => { event.preventDefault(); finish('blocked'); };
            child.postMessage('start');
          } catch { finish('blocked'); }
        });
        setTimeout(() => self.postMessage({ network, imported, nested, violations }), 50);
      }, { capture: true });
      }
    ` + (await response.text()) });
  });
  await page.goto('/test/catalog');
  const origin = new URL(page.url()).origin;
  const outcome = await page.evaluate(async path => {
    const denied = [];
    document.addEventListener('securitypolicyviolation', event => denied.push({ directive: event.effectiveDirective, uri: event.blockedURI }));
    const alternate = await new Promise(resolve => {
      let worker;
      let settled = false;
      const finish = status => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        if (worker) { worker.onmessage = null; worker.onerror = null; worker.terminate(); }
        resolve(status);
      };
      const timer = setTimeout(() => finish('timeout'), 2000);
      try {
        worker = new Worker('/catalog-filter-healthy-worker.js', { type: 'module' });
        worker.onmessage = event => finish(event.data === 'healthy' ? 'allowed' : 'unexpected-message');
        worker.onerror = event => { event.preventDefault(); finish('blocked'); };
        worker.postMessage('start');
      } catch { finish('blocked'); }
    });
    const worker = new Worker(path, { type: 'module' });
    const response = new Promise((resolve, reject) => {
      worker.onmessage = event => resolve(event.data);
      worker.onerror = () => reject(new Error('Owned worker CSP probe failed'));
    });
    worker.postMessage('start');
    try { return { alternate, probe: await response, denied }; }
    finally { worker.terminate(); }
  }, workerPath);
  expect(outcome.alternate).toBe('blocked');
  expect(outcome.denied).toContainEqual({ directive: 'worker-src', uri: `${origin}/catalog-filter-healthy-worker.js` });
  expect(outcome.probe.network).toBe('blocked');
  expect(outcome.probe.imported).toBe('blocked');
  expect(outcome.probe.nested).toBe('blocked');
  expect(outcome.probe.violations).toContainEqual({ directive: 'connect-src', uri: `${origin}/readyz` });
  expect(outcome.probe.violations).toContainEqual({ directive: 'script-src-elem', uri: `${origin}/catalog-filter-healthy-worker.js` });
  expect(outcome.probe.violations).toContainEqual({ directive: 'worker-src', uri: `${origin}/catalog-filter-healthy-worker.js` });
  expect(pageErrors).toEqual([]);
});
