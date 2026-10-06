import { test as base, expect } from '@playwright/test';
import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';

const origin = 'http://127.0.0.1:3000';
const password = 'owned-quick-reply-cooldown-password';
const time = new Date('2026-09-14T12:00:00Z');
const key = '4chan-cd-fixture';
const button = page => page.locator('#quickReply input[type=submit]');

const test = base.extend({
  ownedThread: async ({ request }, use) => {
    const created = await withPostingHistory(() => request.post('/fixture/post', {
      headers: { Origin: origin }, maxRedirects: 0,
      form: { com: 'Owned native Quick Reply cooldown thread', password },
    }));
    expect(created.status()).toBe(303);
    const id = /#p(\d+)$/.exec(created.headers().location)[1];
    try { await use(id); } finally {
      await withDeletionQuota(async () => {
        expect((await request.post('/fixture/delete', { headers: { Origin: origin },
          maxRedirects: 0, form: { no: id, password } })).status()).toBe(303);
      });
    }
  },
});

// This suite deliberately substitutes only the browser's advisory policy with
// owned synthetic 10s/20s values. It never changes a board's database policy.
// Real server projection and authoritative enforcement have separate HTTP tests.
async function open(page, id, { persistent = true, timestamp = null, files = false } = {}) {
  await page.clock.install({ time });
  await page.clock.pauseAt(time);
  await page.addInitScript(({ persistent, timestamp, key }) => {
    localStorage.setItem('4chan-settings', JSON.stringify({ persistentQR: persistent, threadWatcher: false }));
    if (timestamp !== null) localStorage.setItem(key, String(timestamp));
  }, { persistent, timestamp, key });
  await page.route(`**/fixture/thread/${id}`, async route => {
    const response = await route.fetch();
    let found = false;
    let html = (await response.text()).replace(/<form\b[^>]*class="[^"]*\bpostEditor\b[^"]*"[^>]*>/, tag => {
      found = true;
      expect(tag).toMatch(/data-posting-reply-seconds="\d+"/);
      expect(tag).toMatch(/data-posting-image-seconds="\d+"/);
      return tag.replace(/data-posting-reply-seconds="\d+"/, 'data-posting-reply-seconds="10"')
        .replace(/data-posting-image-seconds="\d+"/, 'data-posting-image-seconds="20"')
        .replace(/>$/, ' data-owned-synthetic-cooldown="true">');
    });
    expect(found).toBe(true);
    if (files) {
      // Synthetic upload control, with transport intercepted below. No fabricated
      // capability is ever sent to the server or treated as a real approval.
      html = html.replace('</body>', '<form class="postForm" action="/fixture/upload"><input type="file" name="upfile" accept="image/png"></form></body>');
    }
    await route.fulfill({ response, body: html });
  });
  await page.goto(`/fixture/thread/${id}`);
  await page.locator('.open-qr-link').click();
  await page.locator('#qrCom').fill('Owned cooldown draft');
}

async function timestampEvent(page, value, eventKey = key, area = 'local') {
  await page.evaluate(({ value, eventKey, area }) => {
    if (value !== null) localStorage.setItem(eventKey, String(value));
    window.dispatchEvent(new StorageEvent('storage', {
      key: eventKey, newValue: value === null ? null : String(value),
      storageArea: area === 'local' ? localStorage : sessionStorage,
    }));
  }, { value, eventKey, area });
}

async function rejectPosts(page, body = '{"error":"Owned synthetic rejection"}') {
  const calls = [];
  await page.route('**/fixture/imgboard.php', route => {
    calls.push(route.request());
    return route.fulfill({ contentType: 'application/json', body });
  });
  return calls;
}

for (const persistent of [true, false]) {
  test(`only a confirmed real success records time, with persistent QR ${persistent}`, async ({ page, request, ownedThread }) => {
    await open(page, ownedThread, { persistent });
    expect(await page.evaluate(key => localStorage.getItem(key), key)).toBeNull();
    await withPostingHistory(async () => {
      await button(page).click();
      if (persistent) await expect(page.locator('#qrCom')).toHaveValue('');
      else await expect(page.locator('#quickReply')).toHaveCount(0);
      await expect.poll(() => page.evaluate(key => localStorage.getItem(key), key)).toBe(String(time.getTime()));
    });
    expect((await (await request.get(`/fixture/thread/${ownedThread}.json`)).json()).posts).toHaveLength(2);
    if (persistent) await page.locator('#qrClose').click();
    await page.locator('.open-qr-link').click();
    await expect(button(page)).toHaveValue('10s');
    await expect(button(page)).toBeEnabled();
    expect(await page.evaluate(() => localStorage.getItem('4chan-cd-demo'))).toBeNull();
  });
}

test('ceil rounding, clickable toggle, one-shot automatic real post, and no repeated empty posts', async ({ page, request, ownedThread }) => {
  await open(page, ownedThread, { timestamp: time.getTime() - 1001 });
  await expect(button(page)).toHaveValue('9s');
  await button(page).click(); await expect(button(page)).toHaveValue('9s (auto)');
  await button(page).click(); await expect(button(page)).toHaveValue('9s');
  await button(page).click();
  let posts = 0;
  page.on('request', request => { if (request.url() === `${origin}/fixture/imgboard.php`) posts++; });
  await withPostingHistory(async () => {
    await page.clock.runFor(9000);
    await expect(page.locator('#qrCom')).toHaveValue('');
    await expect(button(page)).toHaveValue('10s');
  });
  expect(posts).toBe(1);
  await page.clock.runFor(30000);
  await expect(button(page)).toHaveValue('Post');
  expect(posts).toBe(1);
  expect((await (await request.get(`/fixture/thread/${ownedThread}.json`)).json()).posts).toHaveLength(2);
});

test('Shift-click bypasses only the advisory and an authoritative error never records time or retries', async ({ page, ownedThread }) => {
  const stamp = time.getTime();
  await open(page, ownedThread, { timestamp: stamp });
  const calls = await rejectPosts(page);
  await button(page).click({ modifiers: ['Shift'] });
  await expect(page.locator('#qrError')).toHaveText('Owned synthetic rejection');
  await expect(button(page)).toHaveValue('10s');
  await expect(page.locator('#qrCom')).toHaveValue('Owned cooldown draft');
  expect(await page.evaluate(key => localStorage.getItem(key), key)).toBe(String(stamp));
  await page.clock.runFor(30000); expect(calls).toHaveLength(1);
});

for (const body of ['{"error":"Owned synthetic rejection"}', '{"tid":1,"pid":2,"unexpected":true}', 'not JSON']) {
  test(`automatic failure disarms without a timestamp or retry: ${body}`, async ({ page, ownedThread }) => {
    const stamp = time.getTime() - 9000;
    await open(page, ownedThread, { timestamp: stamp });
    const calls = await rejectPosts(page, body);
    await button(page).click(); await expect(button(page)).toHaveValue('1s (auto)');
    await page.clock.runFor(1000);
    await expect(page.locator('#qrError')).toHaveText(body.startsWith('{"error":')
      ? 'Owned synthetic rejection' : 'Posting response unavailable. Check the thread before posting again.');
    await expect(button(page)).toHaveValue('Post');
    await expect(page.locator('#qrCom')).toHaveValue('Owned cooldown draft');
    expect(await page.evaluate(key => localStorage.getItem(key), key)).toBe(String(stamp));
    await page.clock.runFor(30000); expect(calls).toHaveLength(1);
  });
}

test('busy click abort takes precedence over Shift bypass and expires without another send', async ({ page, ownedThread }) => {
  await open(page, ownedThread, { timestamp: time.getTime() - 9000 });
  await page.evaluate(() => {
    const original = window.fetch;
    window.ownedPostingCalls = 0;
    window.fetch = (url, options) => {
      if (!String(url).endsWith('/fixture/imgboard.php')) return original(url, options);
      window.ownedPostingCalls++;
      return new Promise((resolve, reject) => options.signal.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError')), { once: true }));
    };
  });
  await button(page).click({ modifiers: ['Shift'] });
  await expect(button(page)).toHaveValue('Sending');
  await timestampEvent(page, time.getTime());
  await expect(button(page)).toHaveValue('Sending');
  await button(page).click({ modifiers: ['Shift'] });
  await expect(page.locator('#qrError')).toHaveText('Posting response unavailable. Check the thread before posting again.');
  await expect(button(page)).toHaveValue('10s');
  await page.clock.runFor(30000);
  expect(await page.evaluate(() => window.ownedPostingCalls)).toBe(1);
});

test('only same-board nonempty localStorage events refresh and future or expired timestamps show Post', async ({ page, ownedThread }) => {
  await open(page, ownedThread, { timestamp: time.getTime() - 5000 });
  await expect(button(page)).toHaveValue('5s');
  await timestampEvent(page, time.getTime(), '4chan-cd-demo');
  await timestampEvent(page, null);
  await timestampEvent(page, '', key);
  await timestampEvent(page, time.getTime(), key, 'session');
  await expect(button(page)).toHaveValue('5s');
  await timestampEvent(page, time.getTime()); await expect(button(page)).toHaveValue('10s');
  await timestampEvent(page, time.getTime() + 1); await expect(button(page)).toHaveValue('Post');
  await timestampEvent(page, time.getTime() - 10000); await expect(button(page)).toHaveValue('Post');
});

for (const replacement of ['edit', 'silent edit', 'quote', 'close/reopen', 'pagehide', 'disable']) {
  test(`armed intent is canceled by ${replacement}`, async ({ page, ownedThread }) => {
    await open(page, ownedThread, { timestamp: time.getTime() });
    const calls = await rejectPosts(page);
    await button(page).click(); await expect(button(page)).toHaveValue('10s (auto)');
    if (replacement === 'edit') await page.locator('#qrCom').fill('Replacement draft');
    if (replacement === 'silent edit') await page.locator('#qrCom').evaluate(input => { input.value = 'Programmatic replacement without input event'; });
    if (replacement === 'quote') await page.locator(`#pi${ownedThread} > .postNum > a[title="Reply to this post"]`).click();
    if (replacement === 'close/reopen') {
      await page.locator('#qrClose').click(); await page.locator('.open-qr-link').click();
      await page.locator('#qrCom').fill('New dialog draft');
      await expect(button(page)).toHaveValue('10s');
    }
    if (replacement === 'pagehide') await page.evaluate(() => window.dispatchEvent(new Event('pagehide')));
    if (replacement === 'disable') await page.evaluate(() => {
      localStorage.setItem('4chan-settings', JSON.stringify({ disableAll: true }));
      window.dispatchEvent(new StorageEvent('storage', { key: '4chan-settings', storageArea: localStorage }));
      window.dispatchEvent(new Event('resize'));
    });
    if (['pagehide', 'disable'].includes(replacement)) await expect(page.locator('#quickReply')).toHaveCount(0);
    await page.clock.runFor(30000);
    expect(calls).toHaveLength(0);
  });
}

test('incoming approved capability chooses image delay and removal returns to text delay', async ({ page, ownedThread }) => {
  await open(page, ownedThread, { timestamp: time.getTime() });
  await page.locator('#quickReply form').evaluate(form => {
    const field = document.createElement('input'); field.type = 'hidden'; field.name = 'upload_id';
    field.value = 'owned-synthetic-capability-never-transmitted'; form.append(field);
    form.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(button(page)).toHaveValue('20s');
  await button(page).click(); await expect(button(page)).toHaveValue('20s (auto)');
  await page.locator('#quickReply form').evaluate(form => {
    form.elements.upload_id.remove(); form.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(button(page)).toHaveValue('10s');
});

for (const selection of ['picker', 'drop']) {
  test(`${selection} replacement disarms and expiry during upload never auto-posts after approval`, async ({ page, ownedThread }) => {
    await open(page, ownedThread, { timestamp: time.getTime() - 9000, files: true });
    const calls = await rejectPosts(page);
    await page.evaluate(thread => {
      const original = window.fetch;
      window.fetch = (url, options) => {
        if (String(url).endsWith('/fixture/upload/cancel')) return Promise.resolve(new Response('{"cancelled":true}', { headers: { 'Content-Type': 'application/json' } }));
        if (!String(url).endsWith('/fixture/upload')) return original(url, options);
        return new Promise((resolve, reject) => {
          window.ownedApproveUpload = () => resolve(new Response(JSON.stringify({
            upload_id: 'a'.repeat(32), upload_capability: 'b'.repeat(64), resto: thread, state: 'approved',
          }), { headers: { 'Content-Type': 'application/json' } }));
          options.signal.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError')), { once: true });
        });
      };
    }, ownedThread);
    await button(page).click(); await expect(button(page)).toHaveValue('1s (auto)');
    if (selection === 'picker') await page.locator('#qrFile').setInputFiles({ name: 'owned.png', mimeType: 'image/png', buffer: Buffer.from('owned synthetic file') });
    else await page.locator('.qr-file-row').evaluate(row => {
      const transfer = new DataTransfer(); transfer.items.add(new File(['owned synthetic file'], 'owned.png', { type: 'image/png' }));
      row.dispatchEvent(new DragEvent('drop', { dataTransfer: transfer, bubbles: true, cancelable: true }));
    });
    await expect(button(page)).toBeDisabled();
    await expect(button(page)).toHaveValue('11s');
    await page.clock.runFor(12000);
    await expect(button(page)).toHaveValue('Post');
    await page.evaluate(() => window.ownedApproveUpload());
    await expect(page.locator('#qrUploadStatus')).toContainText('approved');
    await expect(button(page)).toBeEnabled();
    await page.clock.runFor(10000); expect(calls).toHaveLength(0);
  });
}

test('two tabs keep independent one-shot intent rather than a cross-tab posting lock', async ({ page, context, ownedThread }) => {
  const other = await context.newPage();
  try {
    await open(page, ownedThread, { timestamp: time.getTime() - 9000 });
    await open(other, ownedThread, { timestamp: time.getTime() - 9000 });
    const firstCalls = await rejectPosts(page), secondCalls = await rejectPosts(other);
    await button(page).click(); await button(other).click();
    await expect(button(page)).toHaveValue('1s (auto)');
    await expect(button(other)).toHaveValue('1s (auto)');
    await page.clock.runFor(1000); await other.clock.runFor(1000);
    await expect(page.locator('#qrError')).toHaveText('Owned synthetic rejection');
    await expect(other.locator('#qrError')).toHaveText('Owned synthetic rejection');
    expect(firstCalls).toHaveLength(1); expect(secondCalls).toHaveLength(1);
    await page.clock.runFor(20000); await other.clock.runFor(20000);
    expect(firstCalls).toHaveLength(1); expect(secondCalls).toHaveLength(1);
  } finally { await other.close(); }
});

test('changing the thread target cancels intent before quoting the replacement thread', async ({ page, request, ownedThread }) => {
  const created = await withPostingHistory(() => request.post('/fixture/post', {
    headers: { Origin: origin }, maxRedirects: 0,
    form: { com: 'Owned replacement QR target', password },
  }));
  expect(created.status()).toBe(303);
  const target = /#p(\d+)$/.exec(created.headers().location)[1];
  try {
    await open(page, ownedThread, { timestamp: time.getTime() });
    const calls = await rejectPosts(page);
    // Put another real owned thread's original header on this page, as on a
    // board index; retain its exact source post-number navigation contract.
    const response = await request.get(`/fixture/thread/${target}`);
    expect(response.status()).toBe(200);
    await page.evaluate(({ html, target }) => {
      const parsed = new DOMParser().parseFromString(html, 'text/html');
      const thread = parsed.getElementById(`t${target}`);
      if (!thread) throw new Error('Owned replacement thread missing');
      document.querySelector('.board').append(document.importNode(thread, true));
    }, { html: await response.text(), target });
    await button(page).click(); await expect(button(page)).toHaveValue('10s (auto)');
    await page.locator(`#pi${target} > .postNum > a[title="Reply to this post"]`).click();
    await expect(page.locator('#qrResto')).toHaveValue(target);
    await expect(page.locator('#qrCom')).toHaveValue(`>>${target}\n`);
    await expect(button(page)).toHaveValue('10s');
    await page.clock.runFor(30000); expect(calls).toHaveLength(0);
  } finally {
    await withDeletionQuota(async () => {
      expect((await request.post('/fixture/delete', { headers: { Origin: origin },
        maxRedirects: 0, form: { no: target, password } })).status()).toBe(303);
    });
  }
});

for (const invalidation of ['invalid form', 'closed thread']) {
  test(`expiry refuses an armed ${invalidation} without recording time or retrying`, async ({ page, ownedThread }) => {
    const stamp = time.getTime() - 9000;
    await open(page, ownedThread, { timestamp: stamp });
    const calls = await rejectPosts(page);
    await button(page).click(); await expect(button(page)).toHaveValue('1s (auto)');
    if (invalidation === 'invalid form') {
      // Validity can change without input/change or a changed FormData snapshot.
      await page.locator('#qrCom').evaluate(input => input.setCustomValidity('Owned synthetic validation failure'));
    } else {
      // Match the updater's thread-state attribute and notification contract.
      await page.locator(`#t${ownedThread}`).evaluate(thread => {
        thread.dataset.closed = 'true';
        document.dispatchEvent(new Event('boardThreadStateChanged'));
      });
      await expect(button(page)).toBeDisabled();
      await expect(button(page)).toHaveValue('1s');
    }
    await page.clock.runFor(2000);
    await expect(button(page)).toHaveValue('Post');
    expect(calls).toHaveLength(0);
    expect(await page.evaluate(key => localStorage.getItem(key), key)).toBe(String(stamp));
    await page.clock.runFor(20000); expect(calls).toHaveLength(0);

    // Restoring readiness does not resurrect the old automatic intent, but an
    // explicit click still reaches the normal transport and handles its error.
    if (invalidation === 'invalid form') {
      await page.locator('#qrCom').evaluate(input => input.setCustomValidity(''));
    } else {
      await page.locator(`#t${ownedThread}`).evaluate(thread => {
        thread.dataset.closed = 'false';
        document.dispatchEvent(new Event('boardThreadStateChanged'));
      });
    }
    await expect(button(page)).toBeEnabled();
    await page.clock.runFor(2000); expect(calls).toHaveLength(0);
    await button(page).click();
    await expect(page.locator('#qrError')).toHaveText('Owned synthetic rejection');
    expect(calls).toHaveLength(1);
    expect(await page.evaluate(key => localStorage.getItem(key), key)).toBe(String(stamp));
  });
}
