import { AsyncLocalStorage } from 'node:async_hooks';
import { randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { chmodSync, lstatSync, mkdtempSync, readFileSync, rmSync, rmdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

const manifestVariable = 'BROWSER_DELETION_QUOTA_MANIFEST';
const context = new AsyncLocalStorage();
let queue = Promise.resolve();
// Config modules can be evaluated more than once in the runner. Keep the secret
// only in process memory, never in serialized config metadata or reports.
const state = globalThis[Symbol.for('paperboard.ownedDeletionQuotaRun')] ||= { run: undefined, initialized: false };

function readManifest(filename) {
  const stat = lstatSync(filename);
  const directory = lstatSync(path.dirname(filename));
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 4096 || (process.platform !== 'win32' && (stat.mode & 0o077))
      || !directory.isDirectory() || directory.isSymbolicLink() || (process.platform !== 'win32' && (directory.mode & 0o077))
      || stat.uid !== directory.uid || stat.nlink !== 1) {
    throw new Error('Private owned deletion quota manifest required');
  }
  const manifest = JSON.parse(readFileSync(filename, 'utf8'));
  if (manifest.version !== 1 || !/^[a-f0-9]{64}$/.test(manifest.key)
      || /^0+$/.test(manifest.key) || /^0+$/.test(manifest.marker)
      || !/^[a-f0-9]{64}$/.test(manifest.marker) || !Number.isSafeInteger(manifest.owner_pid)
      || manifest.owner_pid < 1 || typeof manifest.database !== 'string'
      || !manifest.database || manifest.database.length > 63) {
    throw new Error('Invalid owned deletion quota manifest');
  }
  return manifest;
}

function invoke(command) {
  const executable = path.resolve(process.env.CARGO_TARGET_DIR || 'target',
    `debug/examples/deletion-quota-fixture${process.platform === 'win32' ? '.exe' : ''}`);
  const result = spawnSync(executable, [command], {
    encoding: 'utf8', timeout: 15_000,
    // Never pass inherited actor selectors or identity keys to this authority.
    env: {
      APP_ENV: process.env.APP_ENV,
      MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL,
      [manifestVariable]: process.env[manifestVariable],
      PATH: process.env.PATH, SystemRoot: process.env.SystemRoot,
    },
  });
  if (result.error?.code === 'ENOENT') {
    throw new Error('Owned deletion quota fixture is not built; run cargo build -p board-public --example deletion-quota-fixture --features browser-tests --locked');
  }
  if (result.error || result.status !== 0) {
    // Do not echo command environments, credentials, manifest keys or SQL errors.
    throw new Error(`Owned deletion quota fixture ${command} failed`);
  }
}

// Config import prepares identity in memory only: --list and failed server
// startup must not create a manifest or database lease.
export function prepareDeletionQuotaRun() {
  if (state.run) return state.run;
  if (process.env.TEST_WORKER_INDEX !== undefined) {
    const filename = process.env[manifestVariable];
    if (!filename) throw new Error('Owned deletion quota runner manifest is missing');
    state.run = readManifest(filename);
    return state.run;
  }
  if (process.env.APP_ENV !== 'development') {
    throw new Error('Owned deletion quota fixture requires explicit development mode');
  }
  let database;
  try {
    const url = new URL(process.env.MIGRATION_DATABASE_URL);
    if (!['postgres:', 'postgresql:'].includes(url.protocol)
        || url.hostname !== '127.0.0.1' || url.username !== 'board_migrator'
        || url.search || url.hash) throw new Error();
    database = decodeURIComponent(url.pathname.slice(1));
    if (!/^[a-zA-Z0-9_]{1,63}$/.test(database)) throw new Error();
  } catch {
    throw new Error('Owned loopback migration database required');
  }
  delete process.env[manifestVariable];
  state.run = {
    version: 1, key: randomBytes(32).toString('hex'), marker: randomBytes(32).toString('hex'),
    owner_pid: process.pid, database,
  };
  return state.run;
}

// Global setup runs after successful server startup and serial-lane validation.
// Until then no test may mutate the server; init rejects any pre-existing actor.
export function initializeDeletionQuotaRun() {
  const manifest = prepareDeletionQuotaRun();
  if (state.initialized || process.env.TEST_WORKER_INDEX !== undefined) return manifest;
  const directory = mkdtempSync(path.join(tmpdir(), 'owned-browser-deletion-'));
  chmodSync(directory, 0o700);
  const filename = path.join(directory, 'run.json');
  writeFileSync(filename, JSON.stringify(manifest), { flag: 'wx', mode: 0o600 });
  process.env[manifestVariable] = filename;
  try {
    invoke('init');
    state.initialized = true;
    return manifest;
  } catch (error) {
    rmSync(directory, { recursive: true });
    delete process.env[manifestVariable];
    throw error;
  }
}

export function teardownDeletionQuotaRun() {
  const filename = process.env[manifestVariable];
  if (!filename) return;
  const manifest = readManifest(filename);
  if (manifest.owner_pid !== process.pid) throw new Error('Only the owning runner may retire quota fixtures');
  // On failure retain the exact private ownership proof and path for a retry.
  invoke('finish');
  rmSync(filename);
  // Only the dedicated directory containing the exact manifest is removed.
  rmdirSync(path.dirname(filename));
  delete process.env[manifestVariable];
  state.run = undefined;
  state.initialized = false;
}

// One callback may perform at most three successful deletion requests. Failures
// still come from the real server. There is no network interception or POST retry.
// Nested scopes are rejected; independent concurrent scopes are queued.
export async function withDeletionQuota(callback) {
  if (typeof callback !== 'function') throw new TypeError('Deletion quota callback required');
  if (context.getStore()) throw new Error('Nested deletion quota scopes are not allowed');
  if (process.env.VISUAL_FIXTURE_SERVER === '1') return callback();
  const filename = process.env[manifestVariable];
  if (!filename) throw new Error('Owned deletion quota runner manifest is missing');
  readManifest(filename);
  const previous = queue;
  let release;
  queue = new Promise(resolve => { release = resolve; });
  await previous;
  try {
    invoke('reset');
    return await context.run(true, async () => {
      let value;
      const failures = [];
      try { value = await callback(); } catch (error) { failures.push(error); }
      try { invoke('check'); } catch (error) { failures.push(error); }
      if (failures.length === 1) throw failures[0];
      if (failures.length > 1) throw new AggregateError(failures, 'Deletion callback and quota check both failed');
      return value;
    });
  } finally { release(); }
}
