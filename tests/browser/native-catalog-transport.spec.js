import { test, expect } from '@playwright/test';

test('owned catalog alias reuses the real API response and validators without write methods', async ({ request }) => {
  const alias = await request.get('/_watch/demo/catalog.json');
  const api = await request.get('http://127.0.0.1:3003/demo/catalog.json');
  expect(alias.status()).toBe(200);
  expect(api.status()).toBe(200);
  expect(alias.headers()['content-type']).toContain('application/json');
  expect(await alias.text()).toBe(await api.text());
  expect(alias.headers().etag).toBe(api.headers().etag);
  const head = await request.head('/_watch/demo/catalog.json');
  expect(head.status()).toBe(200);
  expect(await head.body()).toHaveLength(0);
  expect(head.headers().etag).toBe(alias.headers().etag);
  const unchanged = await request.get('/_watch/demo/catalog.json', { headers: { 'If-None-Match': alias.headers().etag } });
  expect(unchanged.status()).toBe(304);
  expect(await unchanged.body()).toHaveLength(0);
  for (const method of ['POST', 'PUT', 'DELETE']) {
    expect((await request.fetch('/_watch/demo/catalog.json', { method })).status()).toBe(403);
    expect((await request.fetch('/_watch/demo/catalog.json', {
      method, headers: { Origin: 'http://127.0.0.1:3000' },
    })).status()).toBe(405);
  }
  expect((await request.get('http://127.0.0.1:3003/_watch/demo/catalog.json')).status()).toBe(404);
  expect((await request.get('/_watch/missingcatalogboard/catalog.json')).status()).toBe(404);
});

test('bundled catalog transport uses the existing CSP alias without cookies and retains cooldown', async ({ page, context }) => {
  await page.goto('/demo/');
  const origin = new URL(page.url()).origin;
  await context.addCookies([{ name: 'catalog_transport_probe', value: 'present', url: origin }]);
  const cookies = [];
  await context.route('**/_watch/demo/catalog.json', async route => {
    cookies.push((await route.request().allHeaders()).cookie ?? '');
    await route.continue();
  });
  const result = await page.evaluate(async () => {
    const { NativeCatalogTransport } = await import('/static/native-filter.v1.js');
    const control = (await fetch('/_watch/demo/catalog.json', { credentials: 'include' })).status;
    const transport = new NativeCatalogTransport();
    const cycle = await transport.refresh(['demo']);
    const repeated = await transport.refresh(['demo']);
    return { control, cycle, repeated: repeated.status };
  });
  expect(result.control).toBe(200);
  expect(result.cycle.status).toBe('complete');
  expect(result.cycle.results[0].status).toBe('ok');
  expect(result.cycle.results[0].posts.every(post => typeof post.no === 'string')).toBe(true);
  expect(result.repeated).toBe('cooldown');
  expect(cookies).toHaveLength(2);
  expect(cookies[0]).toContain('catalog_transport_probe=present');
  expect(cookies[1]).not.toContain('catalog_transport_probe');
});
