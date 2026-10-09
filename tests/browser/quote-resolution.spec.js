import { test, expect } from '@playwright/test';
import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';

const origin = 'http://127.0.0.1:3000';

test('persisted quote targets become dead text after owned target deletion without changing their source', async ({ page, request }) => {
  const password = 'owned-source-quote-resolution', threads = [];
  const write = async (resto, com) => {
    const response = await withPostingHistory(() => request.post('/demo/post', {
      headers: { Origin: origin }, maxRedirects: 0,
      form: { resto, com, password, ...(resto === '0' ? { sub: 'Owned quote resolution' } : {}) },
    }));
    expect(response.status()).toBe(303);
    const [, thread, post] = response.headers().location.match(/\/thread\/(\d+)#p(\d+)$/);
    if (resto === '0') threads.push(thread);
    return post;
  };
  const remove = no => withDeletionQuota(async () => {
    const response = await request.post('/demo/delete', {
      headers: { Origin: origin }, maxRedirects: 0, form: { no, password },
    });
    expect(response.status()).toBe(303);
  });
  try {
    const target = await write('0', 'Owned live quote target');
    const targetReply = await write(target, 'Owned live target reply');
    const source = await write('0', `>>${target}\n>>${targetReply}\n>>9223372036854775807\n>>>/fixture/${target}`);
    const local = await write(source, `>>${source}`);
    const path = `/demo/thread/${source}.json`, snapshotPath = `/_watch/demo/thread/${source}/posts`;
    const initial = await request.get(path), initialSnapshot = await request.get(snapshotPath);
    expect(initial.status()).toBe(200); expect(initialSnapshot.status()).toBe(200);
    await page.addInitScript(() => localStorage.setItem('4chan-settings', JSON.stringify({ inlineQuotes: true, quotePreview: true })));
    await page.goto(`/demo/thread/${source}`);
    await expect(page.locator(`#m${local} a.quotelink`)).toHaveAttribute('href', `#p${source}`);
    await expect(page.locator(`#m${source} a.quotelink`)).toHaveCount(2);
    await expect(page.locator(`#m${source} a.quotelink`).last()).toHaveAttribute('href', `/demo/thread/${target}#p${targetReply}`);
    await expect(page.locator(`#m${source} span.deadlink`)).toHaveText('>>9223372036854775807');
    await expect(page.locator(`#m${source}`)).toContainText(`>>>/fixture/${target}`);
    await expect(page.locator(`#m${source} a[href*="/fixture/"]`)).toHaveCount(0);
    await page.locator(`#m${source} a.quotelink`).last().click();
    await expect(page.locator('.inlined .postMessage')).toContainText('Owned live target reply');
    await remove(targetReply);
    for (const [url, before] of [[path, initial], [snapshotPath, initialSnapshot]]) {
      const after = await request.get(url, { headers: {
        'If-None-Match': before.headers().etag,
        'If-Modified-Since': 'Fri, 01 Jan 2100 00:00:00 GMT',
      } });
      expect(after.status()).toBe(200);
      expect(after.headers().etag).not.toBe(before.headers().etag);
      expect(after.headers()['last-modified']).toBeUndefined();
      const payload = await after.json();
      const html = url === path ? payload.posts.map(post => post.com || '').join('')
        : payload.posts.map(post => post.html || '').join('');
      expect(html).toContain(`<span class="deadlink">&gt;&gt;${targetReply}</span>`);
    }
    await page.reload();
    await expect(page.locator(`#m${source} a.quotelink`)).toHaveCount(1);
    await expect(page.locator(`#m${source} span.deadlink`)).toHaveText([`>>${targetReply}`, '>>9223372036854775807']);
    const appended = await write(source, `>>${targetReply}\nNew source after target deletion`);
    await page.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
    await expect(page.locator(`#m${appended} span.deadlink`)).toHaveText(`>>${targetReply}`);
    await expect(page.locator(`#m${appended} a.quotelink`)).toHaveCount(0);
  } finally {
    const cleanupErrors = [];
    for (const id of threads.reverse()) {
      try { await remove(id); } catch (error) { cleanupErrors.push(error); }
    }
    if (cleanupErrors.length) throw new AggregateError(cleanupErrors, 'Owned quote fixture cleanup failed');
  }
});
