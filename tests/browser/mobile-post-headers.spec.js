import { test as base, expect } from '@playwright/test';
import { mobileHeaderLabel } from '../../apps/public/client/native-post-numbers.js';

const origin = 'http://127.0.0.1:3000';
const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-mobile-header-password';
    const name = '<'.repeat(10), subject = 'A'.repeat(31);
    const write = async form => {
      const response = await request.post('/demo/post', { headers: { Origin: origin, Connection: 'close' },
        maxRedirects: 0, form: { name, password, ...form } });
      expect(response.status()).toBe(303); expect(response.headers().connection).toBe('close');
      return response.headers().location.match(/#p(\d+)$/)[1];
    };
    const id = await write({ resto: '0', sub: subject, com: 'Owned mobile header OP' });
    try {
      const reply = await write({ resto: id, com: `>>${id}\nOwned mobile header reply` });
      await use({ id, reply, name, subject, url: `/demo/thread/${id}`, replyTo: form => write({ resto: id, ...form }) });
    } finally {
      const response = await request.post('/demo/delete', { headers: { Origin: origin, Connection: 'close' },
        maxRedirects: 0, form: { no: id, password } });
      expect(response.status()).toBe(303); expect(response.headers().connection).toBe('close');
    }
  },
});

test('persisted script-free mobile headers escape shortened labels and keep native quote submission usable', async ({ browser, request, owned }) => {
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: { width: 390, height: 844 } });
  try {
    const page = await context.newPage(); await page.goto(origin + owned.url);
    await expect(page.locator(`#pim${owned.id}`)).toBeVisible(); await expect(page.locator(`#pi${owned.id}`)).toBeHidden();
    for (const no of [owned.id, owned.reply]) {
      await expect(page.locator(`#pim${no} .name`)).toHaveText(mobileHeaderLabel(owned.name).text);
      await expect(page.locator(`#pim${no} .name`)).toHaveAttribute('title', owned.name);
      await expect(page.locator(`#pi${no} .name`)).toHaveText(owned.name);
      await expect(page.locator(`#pi${no} .name`)).not.toHaveAttribute('title');
      await expect(page.locator(`#pc${no} input[name=password]`)).toHaveCount(1);
      await expect(page.locator(`#pc${no} input[name=password]`)).toHaveValue('');
    }
    await expect(page.locator(`#pim${owned.id} .subject`)).toHaveText(mobileHeaderLabel(owned.subject).text);
    await expect(page.locator(`#pim${owned.id} .subject`)).toHaveAttribute('title', owned.subject);
    await expect(page.locator(`#pim${owned.reply} .subject`)).toHaveCount(0);
    expect(await page.locator('[id]').evaluateAll(nodes => new Set(nodes.map(node => node.id)).size === nodes.length)).toBe(true);
    await page.locator(`#pim${owned.reply} .postNum > a[title="Reply to this post"]`).click();
    await expect(page).toHaveURL(`${origin}${owned.url}?quote=${owned.reply}#reply`);
    await expect(page.locator('#com')).toBeVisible(); await expect(page.locator('#com')).toHaveValue(`>>${owned.reply}\n`);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.locator('#com').fill(`>>${owned.reply}\nOwned script-free mobile submission`);
    await expect(page.locator('#postPassword')).toHaveValue('');
    await page.locator('form.postEditor button[type=submit]').click();
    await expect(page.locator('.postMessage').filter({ hasText: 'Owned script-free mobile submission' })).toHaveCount(1);
    const data = await (await request.get(`${owned.url}.json`)).json();
    expect(data.posts).toHaveLength(3); expect(data.posts[0].name).toBe('&lt;'.repeat(10));
  } finally { await context.close(); }
});

test('one original menu moves between paired headers across viewport and cross-tab mobile preferences', async ({ page, context, owned }) => {
  await page.setViewportSize({ width: 390, height: 844 }); await page.goto(owned.url);
  const trigger = page.locator(`#p${owned.reply} [data-post-menu]`);
  await expect(trigger).toHaveCount(1); await expect(trigger).toBeVisible();
  expect(await trigger.evaluate(node => node.parentElement.id)).toBe(`pim${owned.reply}`);
  expect(await trigger.evaluate(node => node.parentElement.firstElementChild === node)).toBe(true);
  await trigger.evaluate(node => { window.ownedMobileMenu = node; });
  await trigger.click(); await expect(page.locator('#post-menu')).toBeVisible();
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(page.locator('#post-menu')).toHaveCount(0); await expect(page.locator(`#pi${owned.reply} > [data-post-menu]`)).toBeVisible();
  expect(await trigger.evaluate(node => node === window.ownedMobileMenu)).toBe(true);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator(`#pim${owned.reply} > [data-post-menu]`)).toBeVisible();
  const other = await context.newPage();
  try {
    await other.goto(owned.url);
    await other.evaluate(() => { localStorage.setItem('4chan-settings', JSON.stringify({ darkTheme: true }));
      localStorage.setItem('4chan_never_show_mobile', 'true'); });
    await expect(page.locator('body')).toHaveAttribute('data-native-never-mobile', 'true');
    await expect(page.locator(`#pi${owned.reply} > [data-post-menu]`)).toBeVisible();
    await expect(page.locator('body')).not.toHaveClass(/\bm-dark\b/);
    await expect(page.locator('link[data-native-theme-stylesheet]')).toHaveAttribute('href', /theme=tomorrow/);
    await other.evaluate(() => localStorage.removeItem('4chan_never_show_mobile'));
    await expect(page.locator('body')).toHaveClass(/\bm-dark\b/);
    await expect(page.locator(`#pim${owned.reply} > [data-post-menu]`)).toBeVisible();
    await expect(page.locator('link[data-native-theme-stylesheet]')).not.toHaveAttribute('href', /theme=tomorrow/);
    expect(await trigger.evaluate(node => node === window.ownedMobileMenu)).toBe(true);
    await trigger.click(); await expect(page.locator('#post-menu')).toBeVisible();
    await other.evaluate(() => localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true })));
    await expect(page.locator('#post-menu')).toHaveCount(0); await expect(trigger).toBeHidden();
    await expect(page.locator('body')).not.toHaveClass(/\bm-dark\b/);
  } finally { await other.close(); }
});

test('live mobile headers keep number delegation and copies have no post authority', async ({ page, request, owned }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ quotePreview: true, threadStats: false })));
  await page.goto(owned.url);
  const added = await owned.replyTo({ com: `>>${owned.id}\nOwned live mobile reply`, name: 'B'.repeat(31) });
  await page.locator('.threadNav.mobile a[data-cmd=update]').first().click();
  await expect(page.locator(`#pim${added}`)).toBeVisible(); await expect(page.locator(`#pi${added}`)).toBeHidden();
  await expect(page.locator(`#pim${added} .name`)).toHaveText('B'.repeat(30) + '(...)');
  await expect(page.locator(`#pim${added} .name`)).toHaveAttribute('title', 'B'.repeat(31));
  await page.locator(`#pim${added} .postNum > a[title="Reply to this post"]`).click();
  await expect(page.locator('#qrCom')).toHaveValue(`>>${added}\n`);
  await expect(page.locator('#qrResto')).toHaveValue(owned.id);
  await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
  // Keep the target outside the viewport so the real local preview opens.
  await page.locator(`#pc${added}`).evaluate(node => {
    const spacer = document.createElement('div'); spacer.style.height = '1200px'; node.before(spacer);
  });
  await page.locator(`#m${added} .quotelink`).hover();
  const preview = page.locator('#quote-preview'); await expect(preview).toBeVisible();
  await expect(preview.locator('.postInfoM .name')).toHaveText(mobileHeaderLabel(owned.name).text);
  await expect(preview.locator('.postInfoM .name')).toHaveAttribute('title', owned.name);
  await expect(preview.locator('[id], form, input, button, details, script')).toHaveCount(0);
  await expect(preview.locator('.postInfoM .postNum > a')).toHaveCount(2);
  const before = await (await request.get(`${owned.url}.json`)).text();
  await preview.locator('.postInfoM .postNum > a[title="Reply to this post"]').dispatchEvent('click', { button: 0 });
  await expect(page.locator('#quickReply')).toHaveCount(0);
  expect(await (await request.get(`${owned.url}.json`)).text()).toBe(before);
});

test('mobile watching and filters keep full persisted labels behind shortened presentation', async ({ page, owned }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    if (localStorage.getItem('4chan-settings') === null) localStorage.setItem('4chan-settings', JSON.stringify({
      threadWatcher: true, filter: true, threadStats: false,
    }));
  });
  await page.goto(owned.url);
  await page.locator(`#pim${owned.id} > [data-post-menu]`).click();
  await page.getByRole('menuitem', { name: 'Add to watch list', exact: true }).click();
  await expect.poll(() => page.evaluate(id => JSON.parse(localStorage.getItem('4chan-watch') || '{}')[`${id}-demo`]?.[0], owned.id))
    .toBe(owned.subject);
  await page.evaluate(name => localStorage.setItem('4chan-filters', JSON.stringify([
    { type: 1, pattern: name, boards: '', active: true, auto: false, hide: true },
  ])), owned.name);
  await page.reload();
  await expect(page.locator(`#p${owned.reply}`)).toHaveClass(/post-hidden/);
  await expect(page.locator(`#m${owned.reply}`)).toBeHidden();
  await expect(page.locator(`#p${owned.id}`)).not.toHaveClass(/post-hidden/);
  await expect(page.locator(`#pim${owned.id} .name`)).toHaveText(mobileHeaderLabel(owned.name).text);
});
