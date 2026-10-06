import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { watcherSettingsOpener } from './helpers/watcher-settings.js';
import { test as base, expect } from '@playwright/test';

const origin = 'http://127.0.0.1:3000';
const safeCSS = '.reply { background-color: #123456; padding-left: 8px; }';

const test = base.extend({
  owned: async ({ request }, use) => {
    const password = 'owned-native-custom-css-password';
    const write = form => withPostingHistory(() => request.post('/demo/post', {
      headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password },
    }));
    const created = await write({ resto: '0', sub: 'Owned custom CSS', com: 'Original custom CSS post' });
    expect(created.status()).toBe(303);
    const id = created.headers().location.match(/thread\/(\d+)/)[1];
    const replied = await write({ resto: id, com: 'Owned custom CSS reply' });
    expect(replied.status()).toBe(303);
    const reply = replied.headers().location.match(/#p(\d+)/)?.[1];
    expect(reply).toBeTruthy();
    try { await use({ id, reply, url: `/demo/thread/${id}` }); }
    finally {
      await withDeletionQuota(async () => {
        const removed = await request.post('/demo/delete', {
          headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password },
        });
        expect(removed.status()).toBe(303);
      });
    }
  },
});

async function openSettings(page) {
  await watcherSettingsOpener(page).click();
  const dialog = page.getByRole('dialog', { name: 'Settings', exact: true });
  await expect(dialog).toBeVisible();
  const expand = dialog.locator('#settings-expand-all');
  if (await expand.count()) await expand.click();
  return dialog;
}

test('native Custom CSS respects CSP, makes no stylesheet requests and applies only while enabled', async ({ page, owned }) => {
  const attackRequests = [];
  page.on('request', request => {
    if (request.url().includes('native-custom-css-attack')) attackRequests.push(request.url());
  });
  await page.addInitScript(css => {
    localStorage.setItem('4chan-settings', JSON.stringify({ customCSS: true }));
    localStorage.setItem('4chan-css', css);
  }, safeCSS);

  const response = await page.goto(owned.url);
  const csp = response.headers()['content-security-policy'] ?? '';
  const styleDirective = csp.split(';').map(part => part.trim()).find(part => part.startsWith('style-src')) ?? '';
  expect(styleDirective).toMatch(/^style-src\s+'self'(?:\s|$)/);
  expect(styleDirective).not.toMatch(/'unsafe-inline'|data:|blob:/);

  const reply = page.locator(`#p${owned.reply}`);
  await expect.poll(() => reply.evaluate(element => getComputedStyle(element).backgroundColor)).toBe('rgb(18, 52, 86)');
  await expect.poll(() => reply.evaluate(element => getComputedStyle(element).paddingLeft)).toBe('8px');
  expect(await page.locator('style#customCSS').count()).toBe(0);
  expect(attackRequests).toEqual([]);

  const settings = await openSettings(page);
  await settings.locator('#custom-css-edit').click();
  const editor = page.getByRole('dialog', { name: 'Custom CSS', exact: true });
  await expect(editor).toBeVisible();
  await editor.getByLabel('Post CSS', { exact: true }).fill(
    '.reply { background-color: #abcdef; background-image: url(/native-custom-css-attack.png); }',
  );
  await editor.getByRole('button', { name: 'Save CSS', exact: true }).click();
  await expect(editor.getByRole('status')).toContainText(/not allowed|functions are not allowed/i);
  expect(await page.evaluate(() => localStorage.getItem('4chan-css'))).toBe(safeCSS);
  expect(attackRequests).toEqual([]);

  const revised = '.reply { color: #334455; font-size: 16px; margin-top: 4px; }';
  await editor.getByLabel('Post CSS', { exact: true }).fill(revised);
  await editor.getByRole('button', { name: 'Save CSS', exact: true }).click();
  await expect(editor.getByRole('status')).toHaveText('CSS saved.');
  expect(await page.evaluate(() => localStorage.getItem('4chan-css'))).toBe(revised);
  await expect.poll(() => reply.evaluate(element => getComputedStyle(element).color)).toBe('rgb(51, 68, 85)');

  await editor.getByRole('button', { name: 'Cancel', exact: true }).click();
  await settings.getByRole('button', { name: 'Close settings', exact: true }).click();
  await page.evaluate(() => {
    const current = JSON.parse(localStorage.getItem('4chan-settings') || '{}');
    localStorage.setItem('4chan-settings', JSON.stringify({ ...current, customCSS: false }));
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
  });
  await expect.poll(() => reply.evaluate(element => getComputedStyle(element).color)).not.toBe('rgb(51, 68, 85)');

  await page.evaluate(() => {
    const current = JSON.parse(localStorage.getItem('4chan-settings') || '{}');
    localStorage.setItem('4chan-settings', JSON.stringify({ ...current, customCSS: true }));
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings' }));
  });
  await expect.poll(() => reply.evaluate(element => getComputedStyle(element).color)).toBe('rgb(51, 68, 85)');

  await page.evaluate(() => {
    localStorage.setItem('4chan-css', '@import url(/native-custom-css-attack.css); .reply { display: none; }');
    window.dispatchEvent(new StorageEvent('storage', { key: '4chan-css' }));
  });
  await expect(reply).toBeVisible();
  await expect.poll(() => reply.evaluate(element => getComputedStyle(element).color)).not.toBe('rgb(51, 68, 85)');
  expect(attackRequests).toEqual([]);
});
