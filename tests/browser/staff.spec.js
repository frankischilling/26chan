import { test, expect } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { readFileSync, rmSync } from 'node:fs';
import path from 'node:path';
import { createHash, randomBytes } from 'node:crypto';
const binary = process.platform === 'win32' ? '.exe' : '';
function run(name, args, input) {
  const result = spawnSync(path.resolve(`target/debug/${name}${binary}`), args, {
    encoding: 'utf8', timeout: 15_000, input,
    env: { MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL, PATH: process.env.PATH, SystemRoot: process.env.SystemRoot },
  });
  expect(result.status, `Synthetic helper ${name} failed`).toBe(0);
  return result.stdout.trim();
}
function fixture(command, board, input) { const value = run('examples/browser-fixture', [command, board], input); return value ? JSON.parse(value) : null; }
test('synthetic WebAuthn enrollment, login, audited moderation, recovery and logout', async ({ page, context }) => {
  const board = `s${randomBytes(5).toString('hex').slice(0, 9)}`;
  const invitationDir = path.resolve(`.local/staff-browser-${board}`);
  const recoveryDir = path.resolve(`.local/staff-recovery-${board}`);
  const invitationFile = path.join(invitationDir, 'invitation.txt');
  const data = fixture('setup', board);
  try {
    run('staff-operator', ['provision', board, 'moderator', invitationFile]);
    const invitation = readFileSync(invitationFile, 'utf8').trim();
    expect(fixture('inspect', board).credentials).toBe(0);
    const cdp = await context.newCDPSession(page);
    await cdp.send('WebAuthn.enable');
    await cdp.send('WebAuthn.addVirtualAuthenticator', { options: { protocol: 'ctap2', transport: 'usb', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true } });
    expect((await page.request.get('/reports')).status()).toBe(401);
    await page.goto('/');
    await page.getByLabel('Invitation', { exact: true }).fill(invitation);
    await page.getByRole('button', { name: 'Enroll passkey' }).click();
    await expect(page.getByRole('status')).toHaveText('Passkey enrolled. Sign in with your account.');
    expect(fixture('inspect', board).credentials).toBe(1);
    const consumed = await page.evaluate(async token => (await fetch('/enroll/start', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ invitation: token }) })).status, invitation);
    expect(consumed).toBe(401);
    async function login() {
      await page.goto('/'); await page.getByLabel('Account', { exact: true }).fill(board);
      await page.getByRole('button', { name: 'Sign in', exact: true }).click();
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
    const currentAssertion = await page.evaluate(async username => {
      const response = await fetch('/login/start', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username }) });
      if (!response.ok) throw new Error('Synthetic ceremony start failed');
      const challenge = await response.json();
      const publicKey = PublicKeyCredential.parseRequestOptionsFromJSON(challenge.publicKey);
      const credential = await navigator.credentials.get({ publicKey });
      return JSON.stringify(credential.toJSON());
    }, board);
    const currentHandle = (await context.cookies()).find(cookie => cookie.name === 'staff-ceremony').value;
    const expiration = fixture('expire-ceremony', board, createHash('sha256').update(currentHandle).digest('hex'));
    expect(expiration.expired).toBe(1);
    const expiredChallenge = await page.evaluate(async body => (await fetch('/login/finish', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body })).status, currentAssertion);
    expect(expiredChallenge).toBe(401);
    const report = page.locator(`#report-${data.report}`);
    await expect(report).toContainText('Review <b>text</b>');
    expect(await report.locator('b').count()).toBe(0);
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
    const stale = await page.evaluate(async ({ csrf, board, target }) => (await fetch('/moderate', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ csrf, board, target, action: 'close' }) })).status, { csrf, board, target: String(data.thread) });
    expect(stale).toBe(403);
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
    await report.getByRole('button', { name: 'Resolve report', exact: true }).click(); await expect(report).toContainText('resolved');
    await report.getByRole('button', { name: 'Dismiss report', exact: true }).click(); await expect(report).toContainText('dismissed');
    await report.getByRole('button', { name: 'Remove post', exact: true }).click(); await expect(report).toContainText('removed: true');
    // Thread removal remains a real handler even after a report's reply is removed.
    const freshCsrf = await page.locator('input[name=csrf]').first().inputValue();
    const removal = await page.evaluate(async ({ csrf, board, target }) => (await fetch('/moderate', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ csrf, board, target, action: 'remove-thread' }) })).status, { csrf: freshCsrf, board, target: String(data.thread) });
    expect(removal).toBe(200);
    expect((await page.request.get(publicUrl, { headers: { 'If-None-Match': oldEtag } })).status()).toBe(404);
    const persisted = fixture('inspect', board);
    expect(persisted.states).toEqual([[false, false, true]]);
    expect(persisted.audit).toEqual(['close', 'reopen', 'sticky', 'unsticky', 'resolve', 'dismiss', 'remove-post', 'remove-thread']);
    await page.getByRole('button', { name: 'Sign out', exact: true }).click(); await expect(page).toHaveURL('http://localhost:3001/');
    expect((await page.request.get('/reports')).status()).toBe(401);
    expect(fixture('inspect', board).sessions).toBe(0);
    await login(); fixture('expire', board); expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    run('staff-operator', ['role', board, 'admin']);
    expect((await page.request.get('/reports')).status()).toBe(401);
    await login();
    run('staff-operator', ['recover', board, path.join(recoveryDir, 'invitation.txt')]);
    expect((await page.request.get('/reports')).status()).toBe(401);
    expect(fixture('inspect', board).credentials).toBe(0); expect(fixture('inspect', board).sessions).toBe(0);
    const replacement = readFileSync(path.join(recoveryDir, 'invitation.txt'), 'utf8').trim();
    await page.goto('/'); await page.getByLabel('Invitation', { exact: true }).fill(replacement);
    await page.getByRole('button', { name: 'Enroll passkey' }).click();
    await expect(page.getByRole('status')).toHaveText('Passkey enrolled. Sign in with your account.');
    await login();
    run('staff-operator', ['revoke', board]);
    expect((await page.request.get('/reports')).status()).toBe(401);
  } finally {
    fixture('cleanup', board);
    const privateRoot = path.resolve('.local');
    if (path.dirname(invitationDir) !== privateRoot || path.dirname(recoveryDir) !== privateRoot) throw new Error('Invalid fixture cleanup path');
    rmSync(invitationDir, { recursive: true, force: true });
    rmSync(recoveryDir, { recursive: true, force: true });
  }
});
