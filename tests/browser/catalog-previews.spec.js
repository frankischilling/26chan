import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { ownedDeletionMarker, deletionFixture, cleanupDeletionFixtures } from './helpers/deletion-fixture.js';
import { test, expect } from '@playwright/test';

test('persisted hover headers clone nested trips, follow reply deletion and safely render literal user text', async ({ page, request }) => {
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
      const op = await post(board, { sub: marker, name: '<b>Owned OP & name</b>#password', com: '<img src=x onerror=owned()> literal body' });
      created.push({ board, op });
      const first = await post(board, { resto: op, name: 'Older reply', com: 'first owned reply' });
      const last = await post(board, { resto: op, name: '<script>Last & name</script>#password', com: 'last owned reply' });
      const path = `/${board}/catalog?q=${marker}&teaser=off`;
      const target = page.locator(`#thread-${op} ${board === 'news' ? '.txt-date' : '.thumb'}`);
      const source = page.locator(`#thread-${op} template.catalogPreview`);
      const hover = async () => {
        await page.goto(path);
        await source.evaluate(template => {
          window.ownedPreviewSource = template.content.firstElementChild;
          window.ownedPreviewMarkup = template.innerHTML;
        });
        await target.hover();
        await expect(page.locator('#post-preview')).toBeVisible();
        return page.locator('#post-preview');
      };
      let tip = await hover();
      const expectIdentity = async () => {
        await expect(tip).toHaveAttribute('role', 'tooltip');
        await expect(tip.locator(':scope > .post-author')).toHaveText('<b>Owned OP & name</b> !ozOtJW9BFA');
        await expect(tip.locator('.post-last .post-author')).toHaveText('<script>Last & name</script> !ozOtJW9BFA');
        await expect(tip.locator(':scope > .post-author > .post-tripcode')).toHaveText('!ozOtJW9BFA');
        await expect(tip.locator('.post-last > .post-author > .post-tripcode')).toHaveText('!ozOtJW9BFA');
        await expect(tip.locator('.post-tripcode')).toHaveCount(2);
        await expect(tip.locator('.postertrip, [class*="-capcode"]')).toHaveCount(0);
      };
      await expectIdentity();
      // Exercise the actual delegated hover and clone, without synthesizing an
      // author subtree that would hide server-template regressions.
      expect(await source.evaluate(template =>
        template.content.firstElementChild === window.ownedPreviewSource
        && template.innerHTML === window.ownedPreviewMarkup
        && document.querySelector('#post-preview') !== window.ownedPreviewSource)).toBe(true);
      await tip.evaluate(node => { window.ownedFirstPreview = node; });
      await page.mouse.move(0, 0);
      await expect(tip).toHaveCount(0);
      await target.hover();
      await expect(tip).toBeVisible();
      await expectIdentity();
      expect(await tip.evaluate(node => node !== window.ownedFirstPreview && !window.ownedFirstPreview.isConnected)).toBe(true);
      // Let normal observer work settle. A clone must remain inert and stable,
      // rather than acquiring native-board identity wrappers on each mutation.
      expect(await tip.evaluate(async node => {
        const markup = node.innerHTML;
        await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        return node.isConnected && node.innerHTML === markup;
      })).toBe(true);
      expect(await source.evaluate(template => template.innerHTML === window.ownedPreviewMarkup)).toBe(true);
      await expect(tip.locator('.post-last')).toHaveAttribute('data-reply-id', last);
      await expect(tip.locator('.post-teaser')).toHaveText('<img src=x onerror=owned()> literal body');
      await expect(tip.locator('script, img, b')).toHaveCount(0);
      violations.push(...await page.evaluate(() => window.ownedViolations));
      await remove(board, last);
      tip = await hover();
      await expect(tip.locator('.post-last .post-author')).toHaveText('Older reply');
      await expect(tip.locator('.post-last .post-tripcode')).toHaveCount(0);
      await expect(tip.locator(':scope > .post-author > .post-tripcode')).toHaveText('!ozOtJW9BFA');
      await expect(tip.locator('.post-last')).toHaveAttribute('data-reply-id', first);
      await expect(tip).not.toContainText('Last & name');
      await remove(board, first);
      tip = await hover();
      await expect(tip.locator('.post-last')).toHaveCount(0);
      await expect(tip.locator('.post-tripcode')).toHaveCount(1);
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
