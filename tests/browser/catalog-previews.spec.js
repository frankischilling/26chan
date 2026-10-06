import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { ownedDeletionMarker, deletionFixture, cleanupDeletionFixtures } from './helpers/deletion-fixture.js';
import { test, expect } from '@playwright/test';

test('persisted hover headers follow reply deletion and safely render literal user text', async ({ page, request }) => {
  const origin = 'http://127.0.0.1:3000', password = 'owned-preview-password';
  const marker = ownedDeletionMarker(), created = [];
  const errors = [], violations = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => {
    window.ownedViolations = [];
    document.addEventListener('securitypolicyviolation', event => window.ownedViolations.push(event.violatedDirective));
  });
  const post = async (board, form) => {
    const response = await withPostingHistory(() => request.post(`/${board}/post`, { headers: { Origin: origin }, maxRedirects: 0, form: { ...form, password } }));
    expect(response.status()).toBe(303);
    return /#p(\d+)$/.exec(response.headers().location)[1];
  };
  const remove = async (board, id) => {
    if (board === 'news') deletionFixture('age', board, id, marker);
    await withDeletionQuota(async () => {
      const response = await request.post(`/${board}/delete`, { headers: { Origin: origin }, maxRedirects: 0, form: { no: id, password } });
      expect(response.status()).toBe(303);
    });
  };
  try {
    for (const board of ['demo', 'news']) {
      const op = await post(board, { sub: marker, name: '<b>Owned OP & name</b>', com: '<img src=x onerror=owned()> literal body' });
      created.push({ board, op });
      const first = await post(board, { resto: op, name: 'Older reply', com: 'first owned reply' });
      const last = await post(board, { resto: op, name: '<script>Last & name</script>', com: 'last owned reply' });
      const path = `/${board}/catalog?q=${marker}&teaser=off`;
      const hover = async () => {
        await page.goto(path);
        await page.locator(`#thread-${op} ${board === 'news' ? '.txt-date' : '.thumb'}`).hover();
        await expect(page.locator('#post-preview')).toBeVisible();
        return page.locator('#post-preview');
      };
      let tip = await hover();
      await expect(tip.locator(':scope > .post-author')).toHaveText('<b>Owned OP & name</b>');
      await expect(tip.locator('.post-last .post-author')).toHaveText('<script>Last & name</script>');
      await expect(tip.locator('.post-last')).toHaveAttribute('data-reply-id', last);
      await expect(tip.locator('.post-teaser')).toHaveText('<img src=x onerror=owned()> literal body');
      await expect(tip.locator('script, img, b')).toHaveCount(0);
      violations.push(...await page.evaluate(() => window.ownedViolations));
      await remove(board, last);
      tip = await hover();
      await expect(tip.locator('.post-last .post-author')).toHaveText('Older reply');
      await expect(tip.locator('.post-last')).toHaveAttribute('data-reply-id', first);
      await expect(tip).not.toContainText('Last & name');
      await remove(board, first);
      tip = await hover();
      await expect(tip.locator('.post-last')).toHaveCount(0);
      await page.mouse.move(0, 0);
      await expect(tip).toHaveCount(0);
      violations.push(...await page.evaluate(() => window.ownedViolations));
    }
    expect(errors).toEqual([]);
    expect(violations).toEqual([]);
  } finally {
    try {
      for (const { board, op } of created.filter(({ board }) => board === 'demo')) await remove(board, op);
    } finally {
      cleanupDeletionFixtures(created.filter(({ board }) => board === 'news').map(({ board, op }) => ({ board, id: op, marker })));
    }
  }
});
