import { test, expect } from '@playwright/test';
import { createStaffAuthBudget, completeStaffAuthStart, STAFF_AUTH_MAX_WAIT_MS } from './helpers/staff-auth-budget.mjs';
import { withDeletionQuota, withPostingHistory } from './helpers/deletion-quota-fixture.js';
import { spawn, spawnSync } from 'node:child_process';
import { lstatSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import path from 'node:path';
import { createHash, createHmac, randomBytes } from 'node:crypto';
// Shared by browser clicks and direct fetch/API starts across all four cases.
const authBudget = createStaffAuthBudget();
test.beforeAll(async ({}, workerInfo) => {
  // A replacement worker would lose the shared server's earlier start history.
  expect(workerInfo.config.workers, 'Staff authentication pacing requires one worker').toBe(1);
  expect(workerInfo.project.retries, 'Staff authentication pacing requires retries disabled').toBe(0);
  expect(workerInfo.workerIndex, 'Staff authentication pacing requires the original worker').toBe(0);
});
async function startAuthentication(page, kind, action) {
  const response = await authBudget.run(async () => {
    const received = page.waitForResponse(response => response.url() === `http://localhost:3001/${kind}/start`
      && response.request().method() === 'POST');
    return completeStaffAuthStart(received, action);
  });
  expect(response.status(), `Staff ${kind} start status`).toBe(200);
}
const binary = process.platform === 'win32' ? '.exe' : '';
const debugDir = path.resolve(process.env.CARGO_TARGET_DIR || 'target', 'debug');
function run(name, args, input, expected = 0) {
  const result = spawnSync(path.join(debugDir, `${name}${binary}`), args, {
    encoding: 'utf8', timeout: 15_000, input,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot },
  });
  expect(result.status, `Synthetic helper ${name} returned an unexpected status`).toBe(expected);
  return result.stdout.trim();
}
function fixture(command, board, input) { const value = run('examples/browser-fixture', [command, board], input); return value ? JSON.parse(value) : null; }
async function verifyUnstickyOrdering(page, context, publicMediaRequests) {
  // Keep ordering fixtures separate from the primary flow's exact audit/state assertions.
  const board = `s${randomBytes(5).toString('hex').slice(0, 9)}`;
  const publicOrigin = 'http://127.0.0.1:3000';
  const mediaRoot = path.resolve(`.local/staff-media-${board}`);
  let publicPage;
  const data = fixture('setup', board);
  try {
    expect(path.resolve(data.mediaRoot)).toBe(mediaRoot);
    publicPage = await context.newPage();
    publicPage.on('request', request => { if (request.url().startsWith('http://127.0.0.2:3002/')) publicMediaRequests.add(request); });
    const create = async subject => {
      const response = await withPostingHistory(() => page.request.post(`${publicOrigin}/${board}/post`, {
        headers: { Origin: publicOrigin }, maxRedirects: 0,
        form: { resto: '0', sub: subject, com: 'Owned unsticky ordering fixture', password: 'owned-unsticky-password' },
      }));
      expect(response.status()).toBe(303);
      return response.headers().location.match(/thread\/(\d+)/)[1];
    };
    const remainingSticky = await create('Owned remaining sticky');
    const newer = await create('Owned newer ordinary thread');
    const old = String(data.thread);
    const expectOrder = async ids => {
      await publicPage.goto(`${publicOrigin}/${board}/`);
      expect(await publicPage.locator('.board > .thread').evaluateAll(nodes => nodes.map(node => node.id.slice(1)))).toEqual(ids);
      await publicPage.goto(`${publicOrigin}/${board}/catalog?order=alt`);
      await expect(publicPage.locator('#order-ctrl')).toHaveValue('alt');
      expect(await publicPage.locator('#threads > .thread').evaluateAll(nodes => nodes.map(node => node.dataset.threadId))).toEqual(ids);
    };
    await expectOrder([newer, remainingSticky, old]);
    await page.goto('/reports');
    const report = page.locator(`#report-${data.report}`);
    const moderate = async (target, action) => page.request.post('/moderate', {
      maxRedirects: 0, headers: { Origin: 'http://localhost:3001', 'Sec-Fetch-Site': 'same-origin' },
      form: { csrf: await report.locator('input[name=csrf]').first().inputValue(), board, target, action },
    });
    const clicked = async name => {
      const response = page.waitForResponse(response => response.url() === 'http://localhost:3001/moderate' && response.request().method() === 'POST');
      await report.getByRole('button', { name, exact: true }).click();
      expect((await response).status()).toBe(303);
    };
    await clicked('Sticky thread');
    await expect(report).toContainText('sticky: true');
    expect((await moderate(remainingSticky, 'sticky')).status()).toBe(303);
    const before = fixture('inspect', board);
    expect(before.states.map(row => row[1])).toEqual([true, true, false]);
    await clicked('Unsticky thread');
    await expect(report).toContainText('sticky: false');
    const changed = fixture('inspect', board);
    expect(changed.states.map(row => row[1])).toEqual([false, true, false]);
    expect(changed.threadOptions[0][2]).not.toBe(before.threadOptions[0][2]);
    await expectOrder([remainingSticky, old, newer]);
    await expect(publicPage.locator(`#thread-${old}`)).toHaveAttribute('data-sticky', 'false');
    await expect(publicPage.locator(`#thread-${remainingSticky}`)).toHaveAttribute('data-sticky', 'true');
    const newest = await create('Owned ordinary thread after unsticky');
    await expectOrder([remainingSticky, newest, old, newer]);
    expect((await moderate(old, 'unsticky')).status()).toBe(303);
    const repeated = fixture('inspect', board);
    expect(repeated.threadOptions[0][2]).toBe(changed.threadOptions[0][2]);
    expect(repeated.audit).toEqual(changed.audit);
    await expectOrder([remainingSticky, newest, old, newer]);
  } finally {
    try {
      await publicPage?.close();
    } finally {
      try {
        fixture('cleanup', board);
      } finally {
        const privateRoot = path.resolve('.local');
        if (path.dirname(mediaRoot) !== privateRoot || lstatSync(mediaRoot).isSymbolicLink()
            || realpathSync(mediaRoot) !== path.join(realpathSync(privateRoot), `staff-media-${board}`)) throw new Error('Invalid unsticky media cleanup path');
        rmSync(mediaRoot, { recursive: true });
      }
    }
  }
  await page.goto('/reports');
}

async function verifyGroupedOptions(page, context, cdp, publicMediaRequests) {
  // Isolate this sequence from the primary flow's exact legacy audit assertions.
  const board = `s${randomBytes(5).toString('hex').slice(0, 9)}`;
  const publicOrigin = 'http://127.0.0.1:3000';
  const mediaRoot = path.resolve(`.local/staff-media-${board}`);
  const data = fixture('setup', board);
  let publicPage;
  try {
    expect(path.resolve(data.mediaRoot)).toBe(mediaRoot);
    const created = await withPostingHistory(() => page.request.post(`${publicOrigin}/${board}/post`, {
      headers: { Origin: publicOrigin }, maxRedirects: 0,
      form: { resto: '0', sub: 'Owned grouped options peer', com: 'Owned rank ordering fixture', password: 'owned-grouped-options-password' },
    }));
    expect(created.status()).toBe(303);
    const peer = created.headers().location.match(/thread\/(\d+)/)[1];
    publicPage = await context.newPage();
    publicPage.on('request', request => { if (request.url().startsWith('http://127.0.0.2:3002/')) publicMediaRequests.add(request); });
    const expectOrder = async ids => {
      await publicPage.goto(`${publicOrigin}/${board}/catalog?order=alt`);
      expect(await publicPage.locator('#threads > .thread').evaluateAll(nodes => nodes.map(node => node.dataset.threadId))).toEqual(ids);
    };
    await expectOrder([peer, String(data.thread)]);
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: true });
    await page.goto('/reports');
    const report = page.locator(`#report-${data.report}`);
    const optionsLink = report.getByRole('link', { name: 'Thread Options', exact: true });
    await expect(optionsLink).toHaveAttribute('href', `/thread-options?board=${board}&target=${data.thread}`);
    const open = async () => {
      await optionsLink.click();
      await expect(page.getByRole('heading', { name: `Thread Options: /${board}/${data.thread}`, exact: true })).toBeVisible();
      await expect(page.locator('form')).toHaveAttribute('method', 'post');
      await expect(page.locator('form')).toHaveAttribute('action', '/thread-options');
    };
    const save = async () => {
      const [response] = await Promise.all([
        page.waitForResponse(response => response.url() === 'http://localhost:3001/thread-options' && response.request().method() === 'POST'),
        page.getByRole('button', { name: 'Set Options', exact: true }).click(),
      ]);
      expect(response.status()).toBe(303);
      expect(response.headers().location).toBe('/reports');
      await expect(page).toHaveURL(/\/reports$/);
    };
    await open();
    for (const name of ['Sticky', 'Closed', 'Perma-sage', 'Undead']) await expect(page.getByLabel(name, { exact: true })).not.toBeChecked();
    await expect(page.getByLabel('Sticky rank', { exact: true })).toHaveValue('0');
    await expect(page.getByLabel('Perma-age', { exact: true })).toHaveCount(0);
    for (const name of ['Sticky', 'Closed', 'Perma-sage', 'Undead']) await page.getByLabel(name, { exact: true }).check();
    await page.getByLabel('Sticky rank', { exact: true }).fill('10');
    await save();
    const combined = fixture('inspect', board);
    expect(combined.states).toEqual([[true, true, false], [false, false, false]]);
    expect(combined.bumpFlags).toEqual([[true, false], [false, false]]);
    expect(combined.threadOptions[0][1]).toBe(true);
    expect(combined.audit).toEqual(['thread-options']);
    await open();
    for (const name of ['Sticky', 'Closed', 'Perma-sage', 'Undead']) await expect(page.getByLabel(name, { exact: true })).toBeChecked();
    await expect(page.getByLabel('Sticky rank', { exact: true })).toHaveValue('10');
    // Set a competing sticky through the same real grouped handler.
    const csrf = await page.locator('input[name=csrf]').inputValue();
    const peerSaved = await page.request.post('/thread-options', {
      headers: { Origin: 'http://localhost:3001', 'Sec-Fetch-Site': 'same-origin' }, maxRedirects: 0,
      form: { csrf, board, target: peer, sticky: '1', sticky_rank: '20' },
    });
    expect(peerSaved.status()).toBe(303);
    await expectOrder([peer, String(data.thread)]);
    const beforeRank = fixture('inspect', board);
    await page.getByLabel('Sticky rank', { exact: true }).fill('30');
    await save();
    expect(fixture('inspect', board).audit).toEqual(beforeRank.audit);
    await expectOrder([String(data.thread), peer]);
    await open();
    await expect(page.getByLabel('Sticky rank', { exact: true })).toHaveValue('30');
    await save();
    expect(fixture('inspect', board).audit).toEqual(beforeRank.audit);
    await expectOrder([String(data.thread), peer]);
  } finally {
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: false });
    try {
      await publicPage?.close();
    } finally {
      try {
        fixture('cleanup', board);
      } finally {
        const privateRoot = path.resolve('.local');
        if (path.dirname(mediaRoot) !== privateRoot || lstatSync(mediaRoot).isSymbolicLink()
            || realpathSync(mediaRoot) !== path.join(realpathSync(privateRoot), `staff-media-${board}`)) throw new Error('Invalid grouped options media cleanup path');
        rmSync(mediaRoot, { recursive: true });
      }
    }
  }
  await page.goto('/reports');
}

async function verifyPermaageOption(page, board, target, visible) {
  await page.goto(`/thread-options?board=${board}&target=${target}`);
  await expect(page.getByRole('button', { name: 'Set Options', exact: true })).toBeVisible();
  await expect(page.getByLabel('Perma-age', { exact: true })).toHaveCount(visible ? 1 : 0);
  if (visible) await expect(page.getByLabel('Perma-age', { exact: true })).not.toBeChecked();
  await page.getByRole('link', { name: 'Return to report queue', exact: true }).click();
}

test('public spoiler policy controls Quick Reply and forged text choices on desktop and mobile', async ({ page }) => {
  const board = `s${randomBytes(5).toString('hex').slice(0, 9)}`;
  const data = fixture('setup', board), publicOrigin = 'http://127.0.0.1:3000';
  const mediaRoot = path.resolve(`.local/staff-media-${board}`), errors = [];
  expect(path.resolve(data.mediaRoot)).toBe(mediaRoot);
  page.on('pageerror', error => errors.push(error.message));
  try {
    for (const enabled of [false, true]) {
      fixture('public-spoiler-policy', board, enabled ? 'on' : 'off');
      for (const width of [1280, 390]) {
        await page.setViewportSize({ width, height: 900 });
        await page.goto(`${publicOrigin}/${board}/thread/${data.thread}`);
        const source = page.locator('form.postEditor');
        await expect(source).toHaveAttribute('data-spoilers', String(enabled));
        if (width === 390) await page.locator(`#pim${data.thread} > .postNum > a[title="Reply to this post"]`).click();
        else await page.locator('.open-qr-link').click();
        const qr = page.locator('#quickReply');
        await expect(qr.locator('#qrFile')).toBeVisible();
        await expect(qr.locator('[name=spoiler]')).toHaveCount(enabled ? 1 : 0);
        if (enabled) await expect(qr.locator('[name=spoiler]')).toBeDisabled();
        await page.getByRole('button', { name: 'Close Quick Reply', exact: true }).click();
        const posted = await withPostingHistory(() => page.request.post(`${publicOrigin}/${board}/imgboard.php`, {
          headers: { Origin: publicOrigin, Accept: 'application/json' },
          multipart: { mode: 'regist', resto: String(data.thread), pwd: 'owned-public-spoiler-password',
            com: 'Owned forged spoiler text', spoiler: 'false' },
        }));
        expect(posted.status()).toBe(200);
        const result = await posted.json(); expect(result.error).toBeUndefined();
        const json = await (await page.request.get(`${publicOrigin}/${board}/thread/${data.thread}.json`)).json();
        const saved = json.posts.find(post => post.no === result.pid);
        expect(saved.spoiler).toBe(enabled ? 1 : undefined); expect(saved.tim).toBeUndefined();
        expect(saved.com).toBe('Owned forged spoiler text');
      }
    }
    expect(errors).toEqual([]);
  } finally {
    fixture('cleanup', board);
    const privateRoot = path.resolve('.local');
    if (path.dirname(mediaRoot) !== privateRoot || lstatSync(mediaRoot).isSymbolicLink()
        || realpathSync(mediaRoot) !== path.join(realpathSync(privateRoot), `staff-media-${board}`)) throw new Error('Invalid public spoiler media cleanup path');
    rmSync(mediaRoot, { recursive: true });
  }
});

test('Robot9000 cleanup is scoped, script-free, batched and preserves recent history and mutes', async ({ page, context }) => {
  const board = `s${randomBytes(5).toString('hex').slice(0, 9)}`;
  const invitationDir = path.resolve(`.local/staff-cleanup-browser-${board}`);
  const invitationFile = path.join(invitationDir, 'invitation.txt');
  const data = fixture('setup', board);
  const mediaRoot = path.resolve(`.local/staff-media-${board}`);
  const cdp = await context.newCDPSession(page);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    expect(path.resolve(data.mediaRoot)).toBe(mediaRoot);
    fixture('robot9000-seed', board);
    const initial = fixture('robot9000-inspect', board);
    expect(initial).toMatchObject({ texts: 1008, old: 1007, recent: 1, audit: [] });
    expect(initial.mutes).toHaveLength(1);
    run('staff-operator', ['provision', board, 'janitor', invitationFile]);
    run('staff-operator', ['scope', board, board, '-']);
    await cdp.send('WebAuthn.enable');
    await cdp.send('WebAuthn.addVirtualAuthenticator', { options: { protocol: 'ctap2', transport: 'usb', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true } });
    await page.goto('/');
    await page.getByLabel('Invitation', { exact: true }).fill(readFileSync(invitationFile, 'utf8').trim());
    await startAuthentication(page, 'enroll', () => page.getByRole('button', { name: 'Enroll passkey' }).click());
    await expect(page.getByRole('status')).toHaveText('Passkey enrolled. Sign in with your account.');
    const login = async () => {
      await page.goto('/');
      await page.getByLabel('Account', { exact: true }).fill(board);
      await startAuthentication(page, 'login', () => page.getByRole('button', { name: 'Sign in', exact: true }).click());
      await expect(page).toHaveURL(/\/reports$/);
    };
    const review = page.getByRole('button', { name: 'Review originality cleanup', exact: true });
    const denyCleanup = async () => {
      await expect(review).toHaveCount(0);
      expect((await page.request.get(`/robot9000-cleanup?board=${board}`)).status()).toBe(403);
      const csrf = await page.locator('input[name=csrf]').first().inputValue();
      expect((await page.request.post('/robot9000-cleanup', {
        headers: { Origin: 'http://localhost:3001', 'Sec-Fetch-Site': 'same-origin' },
        form: { csrf, board },
      })).status()).toBe(403);
      expect(fixture('robot9000-inspect', board)).toEqual(initial);
    };
    await login();
    await denyCleanup();
    run('staff-operator', ['role', board, 'moderator']);
    await login();
    await denyCleanup();
    // A developer flag only grants cleanup when it is global, not board-scoped.
    run('staff-operator', ['flags', board, 'developer']);
    await login();
    await denyCleanup();
    run('staff-operator', ['scope', board, 'all', '-']);
    await login();
    await expect(review).toBeVisible();
    expect((await page.request.get(`/robot9000-cleanup?board=${board}`)).status()).toBe(200);
    expect(fixture('robot9000-inspect', board)).toEqual(initial);
    run('staff-operator', ['scope', board, 'all', 'noboard']);
    await login();
    await denyCleanup();
    // A scoped manager needs no developer flag and uses the real queue form.
    run('staff-operator', ['role', board, 'manager']);
    run('staff-operator', ['flags', board, '-']);
    run('staff-operator', ['scope', board, board, '-']);
    await login();
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: true });
    await page.reload();
    await page.getByLabel('Originality cleanup board', { exact: true }).fill(board);
    await review.click();
    await expect(page).toHaveURL(new RegExp(`/robot9000-cleanup\\?board=${board}$`));
    await expect(page.getByRole('heading', { name: `Originality history cleanup: /${board}/`, exact: true })).toBeVisible();
    await expect(page.locator('form')).toHaveAttribute('method', 'post');
    await expect(page.locator('form')).toHaveAttribute('action', '/robot9000-cleanup');
    await expect(page.locator('input[name=board]')).toHaveValue(board);
    expect(fixture('robot9000-inspect', board)).toEqual(initial);
    const batch = async (removed, remaining, audit) => {
      const [response] = await Promise.all([
        page.waitForResponse(response => response.url() === 'http://localhost:3001/robot9000-cleanup' && response.request().method() === 'POST'),
        page.getByRole('button', { name: 'Remove expired originality history', exact: true }).click(),
      ]);
      expect(response.status()).toBe(200);
      const submitted = new URLSearchParams(response.request().postData());
      expect([...submitted.keys()].sort()).toEqual(['board', 'csrf']);
      expect(submitted.get('board')).toBe(board);
      expect(submitted.get('csrf')).toBeTruthy();
      await expect(page.getByRole('status')).toContainText(`Removed ${removed} originality records before`);
      await expect(page.getByText(remaining ? 'More eligible records remain. Submit another batch to continue.' : "No eligible records remain at this batch's cutoff.", { exact: true })).toBeVisible();
      expect(fixture('robot9000-inspect', board)).toEqual({ texts: remaining + 1, old: remaining, recent: 1, mutes: initial.mutes, audit });
    };
    await batch(1000, 7, [1000]);
    await batch(7, 0, [1000, 7]);
    await batch(0, 0, [1000, 7, 0]);
    expect(errors).toEqual([]);
  } finally {
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: false });
    fixture('cleanup', board);
    const privateRoot = path.resolve('.local');
    for (const [directory, leaf] of [[invitationDir, `staff-cleanup-browser-${board}`], [mediaRoot, `staff-media-${board}`]]) {
      if (path.dirname(directory) !== privateRoot || lstatSync(directory).isSymbolicLink()
          || realpathSync(directory) !== path.join(realpathSync(privateRoot), leaf)) throw new Error('Invalid Robot9000 cleanup fixture path');
      rmSync(directory, { recursive: true });
    }
  }
});

test('ordinary staff posts keep public IDs, flags, live filtering and password deletion for every rank', async ({ page, context }) => {
  const board = `s${randomBytes(5).toString('hex').slice(0, 9)}`;
  const invitationDir = path.resolve(`.local/staff-ordinary-browser-${board}`);
  const invitationFile = path.join(invitationDir, 'invitation.txt');
  const data = fixture('setup', board);
  const mediaRoot = path.resolve(`.local/staff-media-${board}`);
  expect(path.resolve(data.mediaRoot)).toBe(mediaRoot);
  const publicOrigin = 'http://127.0.0.1:3000';
  const password = 'owned-ordinary-browser-password';
  const publicPage = await context.newPage(), errors = [];
  publicPage.on('pageerror', error => errors.push(error.message));
  try {
    fixture('ordinary-policy', board);
    run('staff-operator', ['provision', board, 'janitor', invitationFile]);
    run('staff-operator', ['scope', board, board, '-']);
    const cdp = await context.newCDPSession(page);
    await cdp.send('WebAuthn.enable');
    await cdp.send('WebAuthn.addVirtualAuthenticator', { options: { protocol: 'ctap2', transport: 'usb', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true } });
    await page.goto('/');
    await page.getByLabel('Invitation', { exact: true }).fill(readFileSync(invitationFile, 'utf8').trim());
    await startAuthentication(page, 'enroll', () => page.getByRole('button', { name: 'Enroll passkey' }).click());
    await expect(page.getByRole('status')).toHaveText('Passkey enrolled. Sign in with your account.');
    for (const [index, role] of ['janitor', 'moderator', 'manager', 'admin'].entries()) {
      if (index > 0) run('staff-operator', ['role', board, role]);
      await page.goto('/'); await page.getByLabel('Account', { exact: true }).fill(board);
      await startAuthentication(page, 'login', () => page.getByRole('button', { name: 'Sign in', exact: true }).click());
      await expect(page).toHaveURL(/\/reports$/);
      await page.getByRole('link', { name: 'Post', exact: true }).click();
      await expect(page).toHaveURL(/\/post$/);
      const submit = async (thread, options, flag, comment, noScript = false) => {
        if (noScript) await cdp.send('Emulation.setScriptExecutionDisabled', { value: true });
        await page.goto(`/post?board=${board}&thread=${thread}`);
        await page.getByLabel('Staff badge', { exact: true }).selectOption('none');
        await page.getByLabel('Name', { exact: true }).fill('Owned ordinary#password');
        await page.getByLabel('Subject', { exact: true }).fill(`Owned ordinary post ${index + 1}`);
        await page.getByLabel('Options', { exact: true }).fill(options);
        await page.getByLabel('Flag', { exact: true }).selectOption(flag);
        await page.getByLabel('Deletion password', { exact: true }).fill(password);
        await page.getByLabel('Comment', { exact: true }).fill(comment);
        const [response] = await Promise.all([
          page.waitForResponse(response => response.url() === 'http://localhost:3001/post' && response.request().method() === 'POST'),
          withPostingHistory(() => page.getByRole('button', { name: 'Post', exact: true }).click()),
        ]);
        if (noScript) await cdp.send('Emulation.setScriptExecutionDisabled', { value: false });
        const reason = response.status() === 303 ? '' : (await response.text()).slice(0, 256);
        expect(response.status(), `Ordinary ${role} posting: ${reason}`).toBe(303);
        await expect(page).toHaveURL(/\/post\?board=.*&thread=.*&posted=[1-9][0-9]*$/);
        return new URL(page.url()).searchParams.get('posted');
      };
      const op = await submit('0', '', '', '[op]Owned opening markup[/op]', true);
      const sage = await submit(op, 'SaGe', '', 'Owned same peer sage reply');
      const flagged = await submit(op, '', 'AC', 'Owned selected board flag');
      const endpoint = `${publicOrigin}/${board}/thread/${op}.json`;
      const rows = (await (await page.request.get(endpoint)).json()).posts;
      const label = rows[0].id;
      expect(label).toMatch(/^[+/0-9A-Za-z]{8}$/);
      expect(rows[0].name).toBe('Owned ordinary'); expect(rows[0].trip).toBe('!ozOtJW9BFA');
      expect(rows[0].country).toBe('XX'); expect(rows[0].country_name).toBe('Unknown');
      expect(rows[0].capcode).toBeUndefined(); expect(rows[1].id).toBe('Heaven');
      expect(rows[2].id).toBe(label); expect(rows[2].board_flag).toBe('AC');
      expect(rows[2].flag_name).toBe('Anarcho-Capitalist'); expect(rows[2].country).toBeUndefined();
      await publicPage.goto(`${publicOrigin}/${board}/thread/${op}`);
      await publicPage.evaluate(() => {
        localStorage.removeItem('4chan-filters');
        localStorage.setItem('4chan-settings', JSON.stringify({ filter: true, threadStats: false }));
      });
      await publicPage.reload();
      await expect(publicPage.locator(`#pi${sage} .posteruid .hand`)).toHaveText('Heaven');
      await expect(publicPage.locator(`#pi${flagged} .bfl-ac`)).toHaveAttribute('title', 'Anarcho-Capitalist');
      await expect(publicPage.locator(`#pi${op} .flag-xx`)).toHaveAttribute('title', 'Unknown');
      for (const width of [1280, 390]) {
        await publicPage.setViewportSize({ width, height: 900 });
        expect(await publicPage.locator(`#p${flagged} .posteruid .hand:visible`).evaluate(element => getComputedStyle(element).backgroundColor)).toMatch(/^rgb\(/);
      }
      const id = publicPage.locator(`#p${op} .posteruid .hand:visible`);
      await id.click(); await expect(id).toHaveAttribute('aria-pressed', 'true');
      await expect(publicPage.locator(`#p${flagged}`)).toHaveClass(/poster-id-highlight/);
      const posted = await withPostingHistory(() => page.request.post(`${publicOrigin}/${board}/post`, { headers: { Origin: publicOrigin }, maxRedirects: 0,
        form: { resto: op, name: 'Owned public peer', email: '', sub: '', com: 'Owned public and staff identity continuity', password } }));
      expect(posted.status()).toBe(303);
      const added = posted.headers().location.match(/#p(\d+)$/)[1];
      await publicPage.locator('.threadNav.mobile a[data-cmd="update"]').first().click();
      await expect(publicPage.locator(`#p${added} .posteruid .hand:visible`)).toHaveText(label);
      await expect(publicPage.locator(`#p${added}`)).toHaveClass(/poster-id-highlight/);
      await id.focus(); await expect(publicPage.locator('#native-poster-id-tip')).toHaveText('3 posts by this ID');
      await publicPage.evaluate(label => localStorage.setItem('4chan-filters', JSON.stringify([{ type: 4, pattern: label, boards: '', active: true, auto: false, hide: true }])), label);
      await publicPage.reload();
      await expect(publicPage.locator(`#p${flagged}`)).toHaveClass(/post-hidden/);
      await expect(publicPage.locator(`#p${added}`)).toHaveClass(/post-hidden/);
      await expect(publicPage.locator(`#p${sage}`)).not.toHaveClass(/post-hidden/);
      await withDeletionQuota(async () => {
        const deleted = await page.request.post(`${publicOrigin}/${board}/delete`, { headers: { Origin: publicOrigin }, maxRedirects: 0, form: { no: flagged, password } });
        expect(deleted.status()).toBe(303);
        expect((await (await page.request.get(endpoint)).json()).posts.some(post => String(post.no) === flagged)).toBe(false);
      });
    }
    expect(fixture('ordinary-inspect', board)).toEqual({ posts: 18, deleted: 4, deletion: 16, contexts: 12, op_peers: 4, op_replies: 8, proofs: 0, audit: 12 });
    expect(errors).toEqual([]);
  } finally {
    await publicPage.close(); fixture('cleanup', board);
    const privateRoot = path.resolve('.local');
    if (path.dirname(invitationDir) !== privateRoot || lstatSync(invitationDir).isSymbolicLink()
        || realpathSync(invitationDir) !== path.join(realpathSync(privateRoot), `staff-ordinary-browser-${board}`)) throw new Error('Invalid ordinary invitation cleanup path');
    rmSync(invitationDir, { recursive: true });
    if (path.dirname(mediaRoot) !== privateRoot || lstatSync(mediaRoot).isSymbolicLink()
        || realpathSync(mediaRoot) !== path.join(realpathSync(privateRoot), `staff-media-${board}`)) throw new Error('Invalid ordinary media cleanup path');
    rmSync(mediaRoot, { recursive: true });
  }
});
test('synthetic WebAuthn enrollment, login, audited moderation, recovery and logout', async ({ page, context }, testInfo) => {
  // This case crosses the shared 30-start budget after the preceding cases.
  // Allow bounded budget waiting without raising other test timeouts.
  test.setTimeout(testInfo.timeout + STAFF_AUTH_MAX_WAIT_MS);
  const board = `s${randomBytes(5).toString('hex').slice(0, 9)}`;
  const invitationDir = path.resolve(`.local/staff-browser-${board}`);
  const recoveryDir = path.resolve(`.local/staff-recovery-${board}`);
  const invitationFile = path.join(invitationDir, 'invitation.txt');
  const data = fixture('setup', board);
  const mediaRoot = path.resolve(`.local/staff-media-${board}`);
  expect(path.resolve(data.mediaRoot)).toBe(mediaRoot);
  const reader = spawn(path.join(debugDir, `board-media-http${binary}`), [], {
    windowsHide: true, stdio: 'ignore',
    env: { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot,
      APP_ENV: 'development', MEDIA_ENABLED: 'false',
      MEDIA_READ_DATABASE_URL: process.env.MEDIA_READ_DATABASE_URL,
      PUBLIC_ORIGIN: 'http://127.0.0.1:3000', STAFF_ORIGIN: 'http://localhost:3001',
      MEDIA_ORIGIN: 'http://127.0.0.2:3002', MEDIA_BIND_ADDR: '127.0.0.2:3002',
      MEDIA_APPROVED_DIR: path.join(mediaRoot, 'objects') },
  });
  let readerError;
  reader.on('error', error => { readerError = error; });
  const mediaRequests = [], staffMediaRequests = [], publicMediaRequests = new Set();
  const mediaWireIds = new Set(), mediaWireHeaders = new Map();
  context.on('request', request => { if (request.url().startsWith('http://127.0.0.2:3002/')) mediaRequests.push({ request, headers: request.allHeaders() }); });
  page.on('request', request => { if (request.url().startsWith(`http://127.0.0.2:3002/${board}/`)) staffMediaRequests.push(request); });
  try {
    await expect.poll(async () => {
      if (readerError || reader.exitCode !== null) throw new Error('Synthetic reader failed to start');
      try { return (await page.request.get('http://127.0.0.2:3002/readyz')).status(); } catch { return 0; }
    }, { timeout: 10_000 }).toBe(200);
    run('staff-operator', ['provision', board, 'moderator', invitationFile]);
    run('staff-operator', ['flags', board, 'capcode,capcodename']);
    const invitation = readFileSync(invitationFile, 'utf8').trim();
    expect(fixture('inspect', board).credentials).toBe(0);
    const cdp = await context.newCDPSession(page);
    await cdp.send('Network.enable');
    cdp.on('Network.requestWillBeSent', event => {
      if (event.request.url.startsWith('http://127.0.0.2:3002/')) mediaWireIds.add(event.requestId);
    });
    cdp.on('Network.requestWillBeSentExtraInfo', event => {
      const names = Object.keys(event.headers).map(key => key.toLowerCase());
      mediaWireHeaders.set(event.requestId, { cookie: names.includes('cookie'), referer: names.includes('referer') });
    });
    await cdp.send('WebAuthn.enable');
    await cdp.send('WebAuthn.addVirtualAuthenticator', { options: { protocol: 'ctap2', transport: 'usb', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true } });
    expect((await page.request.get('/reports')).status()).toBe(401);
    await page.goto('/');
    await page.getByLabel('Invitation', { exact: true }).fill(invitation);
    await startAuthentication(page, 'enroll', () => page.getByRole('button', { name: 'Enroll passkey' }).click());
    await expect(page.getByRole('status')).toHaveText('Passkey enrolled. Sign in with your account.');
    expect(fixture('inspect', board).credentials).toBe(1);
    const consumed = await authBudget.run(() => page.evaluate(async token => (await fetch('/enroll/start', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ invitation: token }) })).status, invitation));
    expect(consumed).toBe(401);
    async function login() {
      await page.goto('/'); await page.getByLabel('Account', { exact: true }).fill(board);
      await startAuthentication(page, 'login', () => page.getByRole('button', { name: 'Sign in', exact: true }).click());
      await expect(page).toHaveURL(/\/reports$/);
    }
    const originalFinish = page.waitForRequest(request => request.url().endsWith('/login/finish'));
    await login();
    const finishRequest = await originalFinish;
    const assertion = finishRequest.postData();
    const originalCookies = await finishRequest.headerValue('cookie');
    const originalHandle = originalCookies.split(';').map(value => value.trim()).find(value => value.startsWith('staff-ceremony=')).slice('staff-ceremony='.length);
    // Restore the exact consumed handle so the database's consume-once check is
    // exercised, rather than merely the missing-cookie guard.
    await context.addCookies([{ name: 'staff-ceremony', value: originalHandle, domain: 'localhost', path: '/', httpOnly: true, sameSite: 'Strict' }]);
    const replay = await page.evaluate(async body => (await fetch('/login/finish', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body })).status, assertion);
    expect(replay).toBe(401);
    const currentAssertion = await authBudget.run(() => page.evaluate(async username => {
      const response = await fetch('/login/start', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username }) });
      if (!response.ok) throw new Error('Synthetic ceremony start failed');
      const challenge = await response.json();
      const publicKey = PublicKeyCredential.parseRequestOptionsFromJSON(challenge.publicKey);
      const credential = await navigator.credentials.get({ publicKey });
      return JSON.stringify(credential.toJSON());
    }, board));
    const currentHandle = (await context.cookies()).find(cookie => cookie.name === 'staff-ceremony').value;
    const expiration = fixture('expire-ceremony', board, createHash('sha256').update(currentHandle).digest('hex'));
    expect(expiration.expired).toBe(1);
    const expiredChallenge = await page.evaluate(async body => (await fetch('/login/finish', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body })).status, currentAssertion);
    expect(expiredChallenge).toBe(401);
    const staffThreadUrl = `http://127.0.0.1:3000/${board}/thread/${data.thread}`;
    run('staff-operator', ['capcode', board, 'founder'], undefined, 1);
    expect(fixture('inspect', board).sessions).toBe(1);
    expect((await page.request.get('/reports')).status()).toBe(200);
    const beforeStaffPost = await page.request.get(staffThreadUrl + '.json');
    fixture('authorized-limit', board, '5000');
    await page.goto(`/post?board=${board}&thread=${data.thread}`);
    await expect(page.getByLabel('Staff badge', { exact: true })).toHaveValue('mod');
    await expect(page.getByLabel('Staff badge', { exact: true }).locator('option')).toHaveText(['None', 'Mod']);
    await expect(page.getByLabel('Highlight administrator post')).toHaveCount(0);
    await expect(page.getByLabel('Name', { exact: true })).toHaveAttribute('maxlength', '255');
    await expect(page.getByLabel('Subject', { exact: true })).toHaveAttribute('maxlength', '255');
    const staffComment = page.getByLabel('Comment', { exact: true });
    await expect(staffComment).toHaveAttribute('maxlength', '10000');
    await staffComment.fill('Owned draft retained across board selection');
    await page.getByLabel('Board', { exact: true }).selectOption('g');
    await expect(staffComment).toHaveAttribute('maxlength', '20000');
    await expect(staffComment).toHaveValue('Owned draft retained across board selection');
    await page.getByLabel('Board', { exact: true }).selectOption(board);
    await expect(staffComment).toHaveAttribute('maxlength', '10000');
    const staffName = 'Owned <staff>' + 'n'.repeat(220);
    await page.getByLabel('Name', { exact: true }).fill(staffName);
    await page.getByLabel('Subject', { exact: true }).fill('s'.repeat(255));
    await staffComment.fill('Script-free staff notice ' + String.fromCodePoint(0x20000).repeat(4900) + ' <script>harmless</script>');
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: true });
    await withPostingHistory(() => page.getByRole('button', { name: 'Post', exact: true }).click());
    await expect(page).toHaveURL(/\/post\?board=.*&thread=.*&posted=[1-9][0-9]*$/);
    const moderatorPost = new URL(page.url()).searchParams.get('posted');
    await expect(page.getByRole('link', { name: 'Open public post' })).toHaveAttribute('href', `${staffThreadUrl}#p${moderatorPost}`);
    const changedStaffPost = await page.request.get(staffThreadUrl + '.json', { headers: { 'If-None-Match': beforeStaffPost.headers().etag } });
    expect(changedStaffPost.status()).toBe(200);
    const persistedModerator = (await changedStaffPost.json()).posts.find(post => String(post.no) === moderatorPost);
    expect(persistedModerator.capcode).toBe('mod'); expect(persistedModerator.name).toBe('Owned &lt;staff&gt;' + 'n'.repeat(220));
    expect(persistedModerator.sub).toBe('s'.repeat(255));
    expect(persistedModerator.com).not.toContain('<script>'); expect(persistedModerator.id).toBeUndefined();
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: false });
    const publicStaffPage = await context.newPage(), publicStaffErrors = [];
    publicStaffPage.on('pageerror', error => publicStaffErrors.push(error.message));
    publicStaffPage.on('request', request => { if (request.url().startsWith('http://127.0.0.2:3002/')) publicMediaRequests.add(request); });
    await publicStaffPage.goto(staffThreadUrl);
    const moderatorIcon = publicStaffPage.locator(`#pi${moderatorPost} .identityIcon`);
    await expect.poll(() => moderatorIcon.evaluate(image => image.complete && image.naturalWidth)).toBe(16);
    await expect(publicStaffPage.locator(`#pi${moderatorPost} .capcode`)).toHaveText('## Mod');
    await page.goto('/reports');
    const report = page.locator(`#report-${data.report}`);
    await expect(report).toContainText('Review <b>text</b>');
    expect(await report.locator('b').count()).toBe(0);
    await expect(report).toContainText('<img src=x onerror=alert(1)> & "fixture".png');
    const thumbnail = report.getByRole('img', { name: 'Attachment thumbnail' });
    await expect(thumbnail).toBeVisible();
    await expect.poll(() => thumbnail.evaluate(image => image.complete && image.naturalWidth)).toBe(250);
    expect(await thumbnail.getAttribute('height')).toBe('150');
    expect(await report.locator('img').count()).toBe(1);
    expect(mediaRequests.length).toBeGreaterThan(0);
    for (const request of mediaRequests) {
      const headers = await request.headers;
      expect(headers.cookie === undefined, 'Media request must omit staff cookies').toBe(true);
      expect(headers.referer === undefined || headers.referer === '' || (publicMediaRequests.has(request.request) && headers.referer === 'http://127.0.0.1:3000/'), 'Only public media requests may disclose the public origin').toBe(true);
    }
    await expect(report.getByRole('button', { name: 'Spoiler image', exact: true })).toHaveCount(0);
    fixture('spoiler-policy', board);
    await page.reload();
    const spoilerUrl = staffThreadUrl + '.json';
    const beforeSpoiler = await page.request.get(spoilerUrl);
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: true });
    await report.getByRole('button', { name: 'Spoiler image', exact: true }).click();
    await expect(report.getByRole('button', { name: 'Remove image spoiler', exact: true })).toBeVisible();
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: false });
    const changedSpoiler = await page.request.get(spoilerUrl, { headers: { 'If-None-Match': beforeSpoiler.headers().etag } });
    expect(changedSpoiler.status()).toBe(200);
    expect((await changedSpoiler.json()).posts.find(post => post.no === data.post).spoiler).toBe(1);
    const spoilerCsrf = await page.locator('input[name=csrf]').first().inputValue();
    const auditBeforeRepeat = fixture('inspect', board).audit;
    const repeatedSpoiler = await page.request.post('/moderate', { maxRedirects: 0,
      headers: { Origin: 'http://localhost:3001', 'Sec-Fetch-Site': 'same-origin' },
      form: { csrf: spoilerCsrf, board, target: String(data.post), action: 'spoiler' } });
    expect(repeatedSpoiler.status()).toBe(303);
    expect((await page.request.get(spoilerUrl, { headers: { 'If-None-Match': changedSpoiler.headers().etag } })).status()).toBe(304);
    expect(fixture('inspect', board).audit).toEqual(auditBeforeRepeat);
    await publicStaffPage.reload();
    await expect(publicStaffPage.locator(`#f${data.post} .imgspoiler > img`)).toHaveAttribute('src', '/static/catalog/spoiler.png');
    await report.getByRole('button', { name: 'Remove image spoiler', exact: true }).click();
    await expect(thumbnail).toBeVisible();
    const unspoiled = await page.request.get(spoilerUrl, { headers: { 'If-None-Match': changedSpoiler.headers().etag } });
    expect(unspoiled.status()).toBe(200);
    expect((await unspoiled.json()).posts.find(post => post.no === data.post).spoiler).toBeUndefined();
    await publicStaffPage.reload();
    await expect(publicStaffPage.locator(`#f${data.post} .imgspoiler`)).toHaveCount(0);
    await report.getByRole('button', { name: 'Spoiler image', exact: true }).click();
    await expect(report.getByRole('button', { name: 'Remove image spoiler', exact: true })).toBeVisible();
    const requestCount = staffMediaRequests.length;
    await page.reload();
    await expect(report.getByRole('link', { name: 'Open spoiler image' })).toBeVisible();
    expect(await report.locator('img').count()).toBe(0);
    expect(staffMediaRequests.length).toBe(requestCount);
    const popupPromise = context.waitForEvent('page');
    await report.getByRole('link', { name: 'Open spoiler image' }).click();
    const popup = await popupPromise;
    await popup.waitForLoadState();
    expect(popup.url()).toBe(`http://127.0.0.2:3002/${board}/${data.tim}.png`);
    expect(await popup.evaluate(() => window.opener === null)).toBe(true);
    await popup.close();
    const full = await page.request.get(`http://127.0.0.2:3002/${board}/${data.tim}.png`);
    expect(full.status()).toBe(200);
    const mediaEtag = full.headers().etag;
    const thumb = await page.request.get(`http://127.0.0.2:3002/${board}/${data.tim}s.jpg`);
    expect(thumb.status()).toBe(200);
    const thumbnailEtag = thumb.headers().etag;
    const cookies = await context.cookies();
    const sessionCookie = cookies.find(c => c.name === 'staff');
    expect(sessionCookie.httpOnly).toBe(true); expect(sessionCookie.sameSite).toBe('Strict'); expect(sessionCookie.domain).toBe('localhost');
    const csrf = await page.locator('input[name=csrf]').first().inputValue();
    const denied = await page.evaluate(async ({ csrf, board, target }) => {
      const send = body => fetch('/moderate', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams(body) }).then(r => r.status);
      return [await send({ csrf: 'invalid', board, target, action: 'close' }), await send({ csrf, board: 'other', target, action: 'close' })];
    }, { csrf, board, target: String(data.thread) });
    expect(denied).toEqual([403, 404]);
    const publicUrl = `http://127.0.0.1:3000/${board}/thread/${data.thread}.json`;
    const before = await page.request.get(publicUrl); expect(before.status()).toBe(200);
    const oldEtag = before.headers().etag; expect(oldEtag).toBeTruthy();
    await report.getByRole('button', { name: 'Close thread', exact: true }).click();
    await expect(report).toContainText('closed: true');
    const changed = await page.request.get(publicUrl, { headers: { 'If-None-Match': oldEtag } });
    expect(changed.status()).toBe(200); expect(changed.headers().etag).not.toBe(oldEtag);
    await report.getByRole('button', { name: 'Reopen thread', exact: true }).click();
    await report.getByRole('button', { name: 'Sticky thread', exact: true }).click();
    await report.getByRole('button', { name: 'Unsticky thread', exact: true }).click();
    await verifyUnstickyOrdering(page, context, publicMediaRequests);
    await verifyGroupedOptions(page, context, cdp, publicMediaRequests);
    fixture('bump-limit', board);
    expect((await (await page.request.get(publicUrl)).json()).posts[0].bumplimit).toBe(1);
    await expect(report.getByRole('button', { name: 'Enable permaage', exact: true })).toHaveCount(0);
    const forgedFlags = await page.evaluate(async ({ csrf, board, target }) => {
      const results = [];
      for (const action of ['permaage', 'unpermaage']) results.push((await fetch('/moderate', {
        method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({ csrf, board, target, action }),
      })).status);
      return results;
    }, { csrf, board, target: String(data.thread) });
    expect(forgedFlags).toEqual([403, 403]);
    fixture('image-limit', board);
    const beforeUndead = await page.request.get(publicUrl);
    expect((await beforeUndead.json()).posts[0].imagelimit).toBe(1);
    const beforeOptions = fixture('inspect', board).threadOptions[0];
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: true });
    await report.getByRole('button', { name: 'Enable Undead', exact: true }).click();
    await expect(report).toContainText('undead: true');
    const undeadResponse = await page.request.get(publicUrl, { headers: { 'If-None-Match': beforeUndead.headers().etag } });
    expect(undeadResponse.status()).toBe(200);
    expect((await undeadResponse.json()).posts[0].imagelimit).toBeUndefined();
    const afterOptions = fixture('inspect', board);
    expect(afterOptions.threadOptions[0][1]).toBe(true);
    expect(afterOptions.threadOptions[0][2]).toBe(beforeOptions[2]);
    expect(afterOptions.threadOptions[0][3]).not.toBe(beforeOptions[3]);
    const duplicateUndead = await page.request.post('/moderate', {
      maxRedirects: 0, headers: { Origin: 'http://localhost:3001', 'Sec-Fetch-Site': 'same-origin' },
      form: { csrf, board, target: String(data.thread), action: 'undead' },
    });
    expect(duplicateUndead.status()).toBe(303);
    const repeatedOptions = fixture('inspect', board);
    expect(repeatedOptions.audit).toEqual(afterOptions.audit);
    expect(repeatedOptions.threadOptions[0][2]).toBe(beforeOptions[2]);
    expect(repeatedOptions.threadOptions[0][3]).not.toBe(afterOptions.threadOptions[0][3]);
    expect((await page.request.get(publicUrl, { headers: { 'If-None-Match': undeadResponse.headers().etag } })).status()).toBe(304);
    await report.getByRole('button', { name: 'Disable Undead', exact: true }).click();
    await expect(report).toContainText('undead: false');
    expect((await (await page.request.get(publicUrl)).json()).posts[0].imagelimit).toBe(1);
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: false });
    const beforeFlags = await page.request.get(publicUrl);
    await report.getByRole('button', { name: 'Enable permasage', exact: true }).click();
    await expect(report).toContainText('permasage: true, permaage: false');
    expect(fixture('inspect', board).bumpFlags).toEqual([[true, false]]);
    const changedFlag = await page.request.get(publicUrl, { headers: { 'If-None-Match': beforeFlags.headers().etag } });
    expect(changedFlag.status()).toBe(304); // Permasage does not change this representation.
    expect((await (await page.request.get(publicUrl)).json()).posts[0].bumplimit).toBe(1);
    run('staff-operator', ['role', board, 'manager']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    await verifyPermaageOption(page, board, data.thread, true);
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: true });
    await report.getByRole('button', { name: 'Enable permaage', exact: true }).click();
    await expect(report).toContainText('permasage: true, permaage: true');
    await report.getByRole('button', { name: 'Disable permaage', exact: true }).click();
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: false });
    run('staff-operator', ['role', board, 'moderator']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    run('staff-operator', ['flags', board, 'developer']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    await verifyPermaageOption(page, board, data.thread, true);
    await report.getByRole('button', { name: 'Enable permaage', exact: true }).click();
    await expect(report).toContainText('permasage: true, permaage: true');
    await report.getByRole('button', { name: 'Disable permaage', exact: true }).click();
    run('staff-operator', ['scope', board, board, '-']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    await verifyPermaageOption(page, board, data.thread, false);
    await expect(report.getByRole('button', { name: 'Enable permaage', exact: true })).toHaveCount(0);
    run('staff-operator', ['scope', board, 'all', '-']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    await expect(report.getByRole('button', { name: 'Enable permaage', exact: true })).toBeVisible();
    run('staff-operator', ['flags', board, '-']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    await expect(report.getByRole('button', { name: 'Enable permaage', exact: true })).toHaveCount(0);
    run('staff-operator', ['role', board, 'admin']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    run('staff-operator', ['capcode', board, 'manager']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login(); await page.goto(`/post?board=${board}&thread=${data.thread}`);
    await expect(page.getByLabel('Highlight administrator post')).toHaveCount(0);
    run('staff-operator', ['capcode', board, 'admin']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login(); await page.goto(`/post?board=${board}&thread=${data.thread}`);
    const administratorSecret = 'owned-private-secure-input';
    const administratorTrip = '!!' + createHmac('sha256', Buffer.alloc(32, 0x11)).update(administratorSecret).digest('base64').slice(0, 11);
    await page.getByLabel('Name', { exact: true }).fill(`Owned administrator##${administratorSecret}`);
    await page.getByLabel('Comment', { exact: true }).fill(`Administrator reply >>${moderatorPost}`);
    await page.getByLabel('Highlight administrator post').check();
    await withPostingHistory(() => page.getByRole('button', { name: 'Post', exact: true }).click());
    await expect(page).toHaveURL(/\/post\?board=.*&thread=.*&posted=[1-9][0-9]*$/);
    const adminPost = new URL(page.url()).searchParams.get('posted');
    await publicStaffPage.locator('.threadNav.desktop a[data-cmd="update"]').first().click();
    await expect(publicStaffPage.locator(`#p${adminPost}`)).toHaveClass(/highlightPost/);
    await expect(publicStaffPage.locator(`#pi${adminPost} .capcode`)).toHaveText('## Admin');
    await expect(publicStaffPage.locator(`#pi${adminPost} .postertrip`)).toHaveText(administratorTrip);
    await expect(publicStaffPage.locator(`#pim${adminPost} .postertrip`)).toHaveText(administratorTrip);
    await expect.poll(() => publicStaffPage.locator(`#pi${adminPost} .identityIcon`).evaluate(image => image.complete && image.naturalWidth)).toBe(16);
    await publicStaffPage.locator(`#p${moderatorPost}`).evaluate(post => { post.hidden = true; });
    await publicStaffPage.locator(`#m${adminPost} a.quotelink`).hover();
    await expect(publicStaffPage.locator('#quote-preview .postInfo .capcode')).toHaveText('## Mod');
    await expect.poll(() => publicStaffPage.locator('#quote-preview .postInfo .identityIcon').evaluate(image => image.complete && image.naturalWidth)).toBe(16);
    await expect(publicStaffPage.locator('#quote-preview')).not.toHaveText(/Staff posting|Deletion password/);
    await publicStaffPage.locator(`#m${data.thread}`).evaluate((message, { board, thread, post }) => {
      const quote = document.createElement('a'); quote.className = 'quotelink';
      quote.href = `/${board}/thread/${thread}#p${post}`; quote.textContent = `>>${post}`;
      message.append(' ', quote);
    }, { board, thread: data.thread, post: adminPost });
    await publicStaffPage.locator(`#m${data.thread} a.quotelink`).last().hover();
    await expect(publicStaffPage.locator('#quote-preview .postInfo .postertrip')).toHaveText(administratorTrip);
    await expect(publicStaffPage.locator('#quote-preview .postInfo .capcode')).toHaveText('## Admin');
    fixture('force-anon', board);
    await publicStaffPage.goto(`http://127.0.0.1:3000/${board}/catalog`);
    await publicStaffPage.locator(`#thread-${data.thread} .thumb`).hover();
    await expect(publicStaffPage.locator('#post-preview .post-last .post-author')).toHaveText(`Owned administrator ${administratorTrip} ## Admin_highlight`);
    await expect(publicStaffPage.locator('#post-preview .post-last .post-author')).toHaveClass('admin_highlight-capcode post-author');
    await expect(publicStaffPage.locator('#post-preview .post-last .post-author > .post-tripcode')).toHaveText(administratorTrip);
    for (const badge of ['admin_highlight', 'founder']) {
      await page.goto(`/post?board=${board}&thread=0`);
      await page.getByLabel('Staff badge', { exact: true }).selectOption(badge);
      await page.getByLabel('Name', { exact: true }).fill(`Owned catalog ${badge}##${administratorSecret}`);
      await page.getByLabel('Subject', { exact: true }).fill('Owned forced subject');
      await page.getByLabel('Comment', { exact: true }).fill('Owned catalog badge predicate');
      await withPostingHistory(() => page.getByRole('button', { name: 'Post', exact: true }).click());
      await expect(page).toHaveURL(/\/post\?board=.*&thread=.*&posted=[1-9][0-9]*$/);
      const op = new URL(page.url()).searchParams.get('posted');
      const stored = (await (await page.request.get(`http://127.0.0.1:3000/${board}/thread/${op}.json`)).json()).posts[0];
      // JSON has the literal admin_hl exception; the catalog uses admin_highlight.
      expect(stored.name).toBe('Anonymous'); expect(stored.trip).toBeUndefined();
      expect(stored.sub).toBeUndefined();
      await publicStaffPage.goto(`http://127.0.0.1:3000/${board}/catalog`);
      const card = publicStaffPage.locator(`#thread-${op} .catalogThumb`);
      const visible = badge === 'admin_highlight';
      await expect(card).toHaveAttribute('data-filter-name', visible ? `Owned catalog ${badge}` : 'Anonymous');
      await expect(card).toHaveAttribute('data-filter-trip', visible ? administratorTrip : '');
      await card.locator('.thumb').hover();
      await expect(publicStaffPage.locator('#post-preview > .post-author')).toHaveText(visible ? `Owned catalog ${badge} ${administratorTrip} ## undefined` : 'Anonymous ## Founder');
      await expect(publicStaffPage.locator('#post-preview > .post-author')).toHaveClass(`${badge}-capcode post-author`);
      await expect(publicStaffPage.locator('#post-preview > .post-author > .post-tripcode')).toHaveCount(visible ? 1 : 0);
      if (visible) await expect(publicStaffPage.locator('#post-preview > .post-author > .post-tripcode')).toHaveText(administratorTrip);
    }
    expect(publicStaffErrors).toEqual([]);
    await publicStaffPage.close();
    const persistedAdmin = (await (await page.request.get(staffThreadUrl + '.json')).json()).posts.find(post => String(post.no) === adminPost);
    expect(persistedAdmin.capcode).toBe('admin_highlight');
    expect(persistedAdmin.trip).toBeUndefined(); expect(persistedAdmin.name).toBe('Anonymous');
    expect(JSON.stringify(persistedAdmin)).not.toContain(administratorSecret);
    await page.goto('/reports');
    // Both controls are ordinary CSRF-protected forms, including without JS.
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: true });
    await report.getByRole('button', { name: 'Enable permaage', exact: true }).click();
    await expect(report).toContainText('permasage: true, permaage: true');
    expect(fixture('inspect', board).bumpFlags).toEqual([[true, true], [false, false], [false, false]]);
    const permaageResponse = await page.request.get(publicUrl, { headers: { 'If-None-Match': beforeFlags.headers().etag } });
    expect(permaageResponse.status()).toBe(200);
    expect((await permaageResponse.json()).posts[0].bumplimit).toBeUndefined();
    await report.getByRole('button', { name: 'Disable permasage', exact: true }).click();
    await report.getByRole('button', { name: 'Disable permaage', exact: true }).click();
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: false });
    await expect(report).toContainText('permasage: false, permaage: false');
    expect(fixture('inspect', board).bumpFlags).toEqual([[false, false], [false, false], [false, false]]);
    expect((await (await page.request.get(publicUrl)).json()).posts[0].bumplimit).toBe(1);
    expect(fixture('age-activity', board).changed).toBe(1);
    const beforeActivity = fixture('session-times', board)[0];
    // Unprotected pages and health checks must not keep a session alive.
    for (const url of ['/', '/staff.js', '/readyz']) expect((await page.request.get(url)).status()).toBe(200);
    expect(fixture('session-times', board)[0]).toEqual(beforeActivity);
    await page.reload(); await expect(report).toBeVisible();
    const afterActivity = fixture('session-times', board)[0];
    expect(afterActivity.slice(0, 2)).toEqual(beforeActivity.slice(0, 2));
    expect(afterActivity[2]).not.toBe(beforeActivity[2]);
    fixture('stale', board);
    await page.reload(); await expect(report).toBeVisible();
    const refreshedCsrf = await page.locator('input[name=csrf]').first().inputValue();
    const stale = await page.evaluate(async ({ csrf, board, target }) => (await fetch('/moderate', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ csrf, board, target, action: 'close' }) })).status, { csrf: refreshedCsrf, board, target: String(data.thread) });
    expect(stale).toBe(403);
    const staleFile = await page.evaluate(async ({ csrf, board, target }) => (await fetch('/moderate', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ csrf, board, target, action: 'remove-file' }) })).status, { csrf: refreshedCsrf, board, target: String(data.post) });
    expect(staleFile).toBe(403);
    const staleSpoiler = await page.evaluate(async ({ csrf, board, target }) => (await fetch('/moderate', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ csrf, board, target, action: 'unspoiler' }) })).status, { csrf: refreshedCsrf, board, target: String(data.post) });
    expect(staleSpoiler).toBe(403);
    await login();
    const idleCsrf = await page.locator('input[name=csrf]').first().inputValue();
    const idleCookie = (await context.cookies()).find(cookie => cookie.name === 'staff').value;
    const beforeIdle = fixture('inspect', board);
    expect(fixture('idle', board).changed).toBe(1);
    const idleTimes = fixture('session-times', board);
    const idleMutation = await page.evaluate(async ({ csrf, board, target }) => (await fetch('/moderate', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ csrf, board, target, action: 'close' }) })).status, { csrf: idleCsrf, board, target: String(data.thread) });
    expect(idleMutation).toBe(401);
    const idlePage = await page.reload();
    expect(idlePage.status()).toBe(401);
    expect(idlePage.headers()['cache-control']).toBe('private, no-store');
    await expect(page.getByText('Authentication required', { exact: true })).toBeVisible();
    expect((await context.cookies()).some(cookie => cookie.name === 'staff')).toBe(true);
    expect(fixture('inspect', board)).toEqual(beforeIdle);
    expect(fixture('session-times', board)).toEqual(idleTimes);
    await login();
    expect((await context.cookies()).find(cookie => cookie.name === 'staff').value !== idleCookie).toBe(true);
    expect(fixture('inspect', board).sessions).toBe(1);
    const attachmentBefore = await page.request.get(publicUrl);
    expect((await attachmentBefore.json()).posts[1].tim).toBe(data.tim);
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: true });
    await report.getByRole('button', { name: 'Remove file only', exact: true }).click();
    await expect(report).toContainText('File unavailable');
    await cdp.send('Emulation.setScriptExecutionDisabled', { value: false });
    await expect(report).toContainText('Harmless reply');
    expect(await report.getByRole('link', { name: 'Open spoiler image' }).count()).toBe(0);
    const attachmentAfter = await page.request.get(publicUrl, { headers: { 'If-None-Match': attachmentBefore.headers().etag } });
    expect(attachmentAfter.status()).toBe(200);
    const deletedFilePost = (await attachmentAfter.json()).posts[1];
    expect(deletedFilePost.filedeleted).toBe(1); expect(deletedFilePost.tim).toBeUndefined();
    expect((await page.request.get(`http://127.0.0.2:3002/${board}/${data.tim}.png`, { headers: { 'If-None-Match': mediaEtag } })).status()).toBe(404);
    expect((await page.request.get(`http://127.0.0.2:3002/${board}/${data.tim}s.jpg`, { headers: { 'If-None-Match': thumbnailEtag } })).status()).toBe(404);
    for (const request of mediaRequests) {
      const headers = await request.headers;
      expect(headers.cookie === undefined, 'Media request must omit staff cookies').toBe(true);
      expect(headers.referer === undefined || headers.referer === '' || (publicMediaRequests.has(request.request) && headers.referer === 'http://127.0.0.1:3000/'), 'Only public media requests may disclose the public origin').toBe(true);
    }
    expect(mediaWireIds.size).toBeGreaterThan(0);
    for (const id of mediaWireIds) {
      const headers = mediaWireHeaders.get(id);
      expect(headers !== undefined, 'Chromium must report media wire headers').toBe(true);
      expect(headers.cookie, 'Media wire request must omit staff cookies').toBe(false);
      expect(headers.referer, 'Media wire request must omit Referer').toBe(false);
    }
    await report.getByRole('button', { name: 'Resolve report', exact: true }).click(); await expect(report).toContainText('resolved');
    await report.getByRole('button', { name: 'Dismiss report', exact: true }).click(); await expect(report).toContainText('dismissed');
    await report.getByRole('button', { name: 'Remove post', exact: true }).click();
    await expect(report).toHaveCount(0);
    expect(fixture('inspect', board).reports, 'Whole-post deletion removes the report from storage as well as the queue').toBe(0);
    // Thread removal remains a real handler even after a report's reply is removed.
    const freshCsrf = await page.locator('input[name=csrf]').first().inputValue();
    const removal = await page.evaluate(async ({ csrf, board, target }) => (await fetch('/moderate', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ csrf, board, target, action: 'remove-thread' }) })).status, { csrf: freshCsrf, board, target: String(data.thread) });
    expect(removal).toBe(200);
    expect((await page.request.get(publicUrl, { headers: { 'If-None-Match': oldEtag } })).status()).toBe(404);
    const persisted = fixture('inspect', board);
    expect(persisted.reports).toBe(0);
    expect(persisted.states).toEqual([[false, false, true], [false, false, false], [false, false, false]]);
    expect(persisted.audit).toEqual(['staff-post', 'spoiler', 'unspoiler', 'spoiler', 'close', 'reopen', 'sticky', 'unsticky', 'undead', 'unundead', 'permasage', 'permaage', 'unpermaage', 'permaage', 'unpermaage', 'staff-post', 'staff-post', 'staff-post', 'permaage', 'unpermasage', 'unpermaage', 'remove-file', 'resolve', 'dismiss', 'remove-post', 'remove-thread']);
    await page.getByRole('button', { name: 'Sign out', exact: true }).click(); await expect(page).toHaveURL('http://localhost:3001/');
    expect((await page.request.get('/reports')).status()).toBe(401);
    expect(fixture('inspect', board).sessions).toBe(0);
    await login(); fixture('expire', board); expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    run('staff-operator', ['role', board, 'moderator']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    run('staff-operator', ['recover', board, path.join(recoveryDir, 'invitation.txt')]);
    expect((await page.request.get('/reports')).status()).toBe(401);
    expect(fixture('inspect', board).credentials).toBe(0); expect(fixture('inspect', board).sessions).toBe(0);
    const replacement = readFileSync(path.join(recoveryDir, 'invitation.txt'), 'utf8').trim();
    await page.goto('/'); await page.getByLabel('Invitation', { exact: true }).fill(replacement);
    let replacementFinish;
    await startAuthentication(page, 'enroll', () => {
      // Start the response timeout after admission, so it excludes budget waiting.
      replacementFinish = page.waitForResponse(response => response.url() === 'http://localhost:3001/enroll/finish'
        && response.request().method() === 'POST');
      // Retain the original rejection for the finish assertion, while owning it
      // if an earlier click or start-status assertion fails.
      replacementFinish.catch(() => {});
      return page.getByRole('button', { name: 'Enroll passkey' }).click();
    });
    expect((await replacementFinish).status(), 'Replacement enrollment finish status').toBe(200);
    await expect(page.getByRole('status')).toHaveText('Passkey enrolled. Sign in with your account.');
    await login();
    run('staff-operator', ['revoke', board]);
    expect((await page.request.get('/reports')).status()).toBe(401);
  } finally {
    if (reader.exitCode === null && reader.signalCode === null && !readerError) {
      const stopped = new Promise(resolve => reader.once('exit', resolve));
      reader.kill();
      await stopped;
    }
    fixture('cleanup', board);
    const privateRoot = path.resolve('.local');
    if (path.dirname(invitationDir) !== privateRoot || path.dirname(recoveryDir) !== privateRoot) throw new Error('Invalid fixture cleanup path');
    rmSync(invitationDir, { recursive: true, force: true });
    rmSync(recoveryDir, { recursive: true, force: true });
    if (path.dirname(mediaRoot) !== privateRoot || lstatSync(mediaRoot).isSymbolicLink()
        || realpathSync(mediaRoot) !== path.join(realpathSync(privateRoot), `staff-media-${board}`)) throw new Error('Invalid media fixture cleanup path');
    rmSync(mediaRoot, { recursive: true });
  }
});
