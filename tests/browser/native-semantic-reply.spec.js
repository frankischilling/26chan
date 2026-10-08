import { test, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import path from 'node:path';
import { withPostingHistory } from './helpers/deletion-quota-fixture.js';

const origin = 'http://127.0.0.1:3000';
const cases = [
  { name: 'subject', sub: 'Owned semantic subject', com: 'Comment must not replace the subject.', semantic: 'owned-semantic-subject' },
  { name: 'comment fallback', sub: '!!!', com: 'Owned comment fallback\nIgnored later line', semantic: 'owned-comment-fallback' },
  { name: 'empty context', sub: '!!!', com: '!!!', semantic: undefined },
];

// Existing owned fixture: two threads per page, isolated from imported/user posts.
function fixture(command, board) {
  const binary = path.resolve(process.env.CARGO_TARGET_DIR || 'target', `debug/examples/navigation-fixture${process.platform === 'win32' ? '.exe' : ''}`);
  const result = spawnSync(binary, [command, board], { encoding: 'utf8', timeout: 15000,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot } });
  expect(result.error, 'Owned navigation fixture must launch').toBeUndefined();
  expect(result.status, 'Owned navigation fixture must succeed').toBe(0);
}

async function createThread(request, board, fields) {
  const response = await withPostingHistory(() => request.post(`${origin}/${board}/post`, {
    headers: { Origin: origin }, maxRedirects: 0,
    form: { resto: '0', password: 'owned-semantic-reply-password', ...fields },
  }));
  expect(response.status()).toBe(303);
  const match = /#p(\d+)$/.exec(response.headers().location);
  expect(match).not.toBeNull();
  return match[1];
}

async function followReply(page, board, id, href) {
  const header = page.locator(`#pi${id}`);
  const reply = header.getByRole('link', { name: 'Reply', exact: true });
  await expect(reply).toHaveAttribute('href', href);
  // Semantic contexts belong only to the plain OP navigation link.
  await expect(header.getByTitle('Link to this post', { exact: true })).toHaveAttribute('href', `/${board}/thread/${id}#p${id}`);
  await expect(header.getByTitle('Reply to this post', { exact: true })).toHaveAttribute('href', `/${board}/thread/${id}?quote=${id}#reply`);
  await reply.click();
  await expect(page).toHaveURL(`${origin}${href}`);
  await expect(page.locator('.board > .thread')).toHaveCount(1);
  await expect(page.locator(`#t${id} #p${id}`)).toBeVisible();
  await expect(page.locator('form.postEditor input[name="resto"]')).toHaveValue(id);
}

for (const javaScriptEnabled of [false, true]) for (const sample of cases) {
  test(`OP Reply follows exact ${sample.name} context from board and shared depager (JavaScript ${javaScriptEnabled})`, async ({ browser }) => {
    test.setTimeout(120000);
    const board = `dp${randomBytes(4).toString('hex')}`;
    fixture('setup', board);
    let context;
    try {
      context = await browser.newContext({ javaScriptEnabled, viewport: { width: 1280, height: 900 } });
      const request = context.request, page = await context.newPage();
      const id = await createThread(request, board, { sub: sample.sub, com: sample.com });
      await createThread(request, board, { sub: 'Newer first thread', com: 'Owned newer first comment.' });
      await createThread(request, board, { sub: 'Newer second thread', com: 'Owned newer second comment.' });
      const response = await request.get(`${origin}/${board}/thread/${id}.json`);
      expect(response.status()).toBe(200);
      const { posts } = await response.json();
      expect(String(posts[0].no)).toBe(id);
      // Pin source semantics as well as HTML/JSON agreement: a broken shared
      // projection must not make both sides of this assertion silently agree.
      expect(posts[0].semantic_url).toBe(sample.semantic);
      if (sample.semantic === undefined) expect(posts[0]).not.toHaveProperty('semantic_url');
      const href = `/${board}/thread/${id}${posts[0].semantic_url ? `/${posts[0].semantic_url}` : ''}`;
      await page.goto(`${origin}/${board}/1`);
      await followReply(page, board, id, href);

      if (javaScriptEnabled) {
        await page.goto(`${origin}/${board}/0`);
        await expect(page.locator(`#t${id}`)).toHaveCount(0);
        const next = page.waitForResponse(value => new URL(value.url()).pathname === `/_watch/${board}/page/1`);
        await page.locator('#depage').click();
        const snapshotResponse = await next;
        expect(snapshotResponse.status()).toBe(200);
        const snapshot = await snapshotResponse.json();
        const thread = snapshot.threads.find(value => value.thread === id);
        expect(thread).toBeDefined();
        expect(thread.posts[0].html).toContain(`[<a href="${href}">Reply</a>]`);
        await expect(page.locator(`#t${id}`)).toBeVisible();
        await followReply(page, board, id, href);
      }
    } finally {
      try { if (context) await context.close(); }
      finally { fixture('cleanup', board); }
    }
  });
}
