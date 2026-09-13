import { test, expect } from '@playwright/test';

const workerPath = '/static/native-filter.v1.js';

test('native filter module workers match exact IDs, terminate on deadlines and cancel without blocking the page', async ({ page }) => {
  const response = await page.goto('/demo/');
  const origin = new URL(page.url()).origin;
  expect(response.headers()['content-security-policy']).toContain(`worker-src ${origin}${workerPath};`);
  const outcome = await page.evaluate(async path => {
    const { NativeFilterMatcher } = await import(path);
    let created = 0, terminated = 0, ticks = 0;
    const createWorker = () => {
      const worker = new Worker(path, { type: 'module' });
      created++;
      const stop = worker.terminate.bind(worker);
      worker.terminate = () => { terminated++; stop(); };
      return worker;
    };
    const engine = new NativeFilterMatcher({ createWorker });
    const row = { type: 5, pattern: '/paper/i', boards: 'demo', active: true };
    const matched = await engine.match([row], 'demo', [{ no: '9007199254740992', sub: 'PAPER' },
      { no: '9007199254740993', sub: 'other' }, { no: '9223372036854775807', sub: 'paper' }]);
    const invalid = await engine.match([{ ...row, pattern: '/[/' }], 'demo', []);
    const timer = setInterval(() => ticks++, 10);
    const slow = { ...row, type: 2, pattern: '/(a+)+$/' };
    const posts = [{ no: '1', comment: 'a'.repeat(48) + '!' }];
    const timed = await new NativeFilterMatcher({ createWorker, deadline: 150 }).match([slow], 'demo', posts);
    clearInterval(timer);
    const controller = new AbortController();
    const pending = engine.match([slow], 'demo', posts, { signal: controller.signal });
    controller.abort();
    const cancelled = await pending;
    const emptyText = await engine.match([{ ...row, type: 2, pattern: '/^$/' }], 'demo',
      [{ no: '1', comment: '' }, { no: '2' }, { no: '3', comment: 'text' }]);
    return { matched, invalid, timed, cancelled, emptyText, ticks, created, terminated };
  }, workerPath);
  expect(outcome.matched).toEqual({ status: 'ok', matches: [
    { id: '9007199254740992', filter: 0 }, { id: '9223372036854775807', filter: 0 },
  ] });
  expect(outcome.invalid).toEqual({ status: 'invalid-filter', index: 0 });
  expect(outcome.timed.status).toBe('timeout');
  expect(outcome.cancelled.status).toBe('cancelled');
  expect(outcome.emptyText).toEqual({ status: 'ok', matches: [{ id: '1', filter: 0 }] });
  expect(outcome.ticks).toBeGreaterThan(0);
  expect(outcome.created).toBe(5);
  expect(outcome.terminated).toBe(5);
});

test('actual worker response CSP denies network, imports and nested workers with healthy browser controls', async ({ page, context, request }) => {
  await context.route('**/native-filter-control', route => route.fulfill({
    contentType: 'text/html', body: '<!doctype html><p>Owned worker control</p>',
  }));
  await context.route('**/native-filter-healthy-worker.js', route => route.fulfill({
    contentType: 'text/javascript', body: 'self.onmessage = () => self.postMessage("healthy");',
  }));
  await page.goto('/native-filter-control');
  const healthy = await page.evaluate(async () => {
    const worker = new Worker('/native-filter-healthy-worker.js', { type: 'module' });
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
  // Use the real release response headers with a benign owned probe body.
  await context.route(`**${workerPath}`, async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `
      const violations = [];
      self.addEventListener('securitypolicyviolation', event => violations.push({
        directive: event.effectiveDirective, uri: event.blockedURI
      }));
      self.onmessage = async () => {
        const network = await fetch('/readyz').then(() => 'allowed', () => 'blocked');
        const imported = await import('/native-filter-healthy-worker.js').then(() => 'allowed', () => 'blocked');
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
            child = new Worker('/native-filter-healthy-worker.js', { type: 'module' });
            child.onmessage = event => finish(event.data === 'healthy' ? 'allowed' : 'unexpected-message');
            child.onerror = event => { event.preventDefault(); finish('blocked'); };
            child.postMessage('start');
          } catch { finish('blocked'); }
        });
        setTimeout(() => self.postMessage({ network, imported, nested, violations }), 50);
      };
    ` });
  });
  await page.goto('/demo/');
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
        worker = new Worker('/native-filter-healthy-worker.js', { type: 'module' });
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
  expect(outcome.denied).toContainEqual({ directive: 'worker-src', uri: `${origin}/native-filter-healthy-worker.js` });
  expect(outcome.probe.network).toBe('blocked');
  expect(outcome.probe.imported).toBe('blocked');
  expect(outcome.probe.nested).toBe('blocked');
  expect(outcome.probe.violations).toContainEqual({ directive: 'connect-src', uri: `${origin}/readyz` });
  expect(outcome.probe.violations).toContainEqual({ directive: 'script-src-elem', uri: `${origin}/native-filter-healthy-worker.js` });
  expect(outcome.probe.violations).toContainEqual({ directive: 'worker-src', uri: `${origin}/native-filter-healthy-worker.js` });
});
