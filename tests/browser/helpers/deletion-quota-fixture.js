import { AsyncLocalStorage } from 'node:async_hooks';
import { randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { accessSync, constants, chmodSync, lstatSync, mkdtempSync, readFileSync, rmSync, rmdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

const manifestVariable = 'BROWSER_DELETION_QUOTA_MANIFEST';
const context = new AsyncLocalStorage();
let queue = Promise.resolve();
// Config modules can be evaluated more than once in the runner. Keep the secret
// only in process memory, never in serialized config metadata or reports.
const state = globalThis[Symbol.for('paperboard.ownedDeletionQuotaRun')] ||= { run: undefined, initialized: false, pendingInitialization: false };

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

function executablePath() {
  return path.resolve(process.env.CARGO_TARGET_DIR || 'target',
    `debug/examples/deletion-quota-fixture${process.platform === 'win32' ? '.exe' : ''}`);
}

function missingExecutable() {
  return new Error('Owned deletion quota fixture is not built; run cargo build -p board-public --example deletion-quota-fixture --features browser-tests --locked');
}

function invoke(command, args = []) {
  const executable = executablePath();
  const result = spawnSync(executable, [command, ...args], {
    encoding: 'utf8', timeout: 15_000, maxBuffer: 262_144,
    // Never pass inherited actor selectors or identity keys to this authority.
    env: {
      APP_ENV: process.env.APP_ENV,
      MIGRATION_DATABASE_URL: process.env.MIGRATION_DATABASE_URL,
      [manifestVariable]: process.env[manifestVariable],
      PATH: process.env.PATH, SystemRoot: process.env.SystemRoot,
    },
  });
  if (result.error?.code === 'ENOENT') {
    throw missingExecutable();
  }
  if (result.error || result.status !== 0) {
    // Do not echo command environments, credentials, manifest keys or SQL errors.
    throw new Error(`Owned deletion quota fixture ${command} failed`);
  }
  return result.stdout;
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
  if (state.pendingInitialization) {
    throw new Error('Owned deletion quota initialization outcome is uncertain; retire the exact pending run before initializing again');
  }
  if (state.initialized || process.env.TEST_WORKER_INDEX !== undefined) return manifest;
  // A known missing binary must leave no ownership file or database lease.
  try { accessSync(executablePath(), constants.X_OK); } catch { throw missingExecutable(); }
  const directory = mkdtempSync(path.join(tmpdir(), 'owned-browser-deletion-'));
  chmodSync(directory, 0o700);
  const filename = path.join(directory, 'run.json');
  writeFileSync(filename, JSON.stringify(manifest), { flag: 'wx', mode: 0o600 });
  process.env[manifestVariable] = filename;
  // Once init is launched, failure can follow a committed lease but precede its
  // acknowledgement. Keep the exact proof and refuse a blind second init.
  state.pendingInitialization = true;
  invoke('init');
  state.pendingInitialization = false;
  state.initialized = true;
  return manifest;
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
  state.pendingInitialization = false;
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

// An explicit independent posting action starts with only this run's history
// cleared. Identity, deletion quota and Robot9000 state remain unchanged. Keep
// actual cooldown scenarios together inside one callback, without inner scopes.
const postingContext = new AsyncLocalStorage();
let postingQueue = Promise.resolve();
export async function withPostingHistory(callback) {
  if (typeof callback !== 'function') throw new TypeError('Posting history callback required');
  if (postingContext.getStore()) throw new Error('Nested posting history scopes are not allowed');
  if (process.env.VISUAL_FIXTURE_SERVER === '1') return callback();
  const filename = process.env[manifestVariable];
  if (!filename) throw new Error('Owned deletion quota runner manifest is missing');
  readManifest(filename);
  const previous = postingQueue;
  let release;
  postingQueue = new Promise(resolve => { release = resolve; });
  await previous;
  try {
    invoke('reset-posting');
    return await postingContext.run(true, callback);
  } finally { release(); }
}

// Catalog activation is global to this disposable database. This queue only
// serializes scopes in this worker; the Rust lease rejects other fixture runs.
// Other workloads must have their own exclusive disposable database.
const catalogState = globalThis[Symbol.for('paperboard.ownedReportCatalogScopes')] ||= {
  context: new AsyncLocalStorage(), queue: Promise.resolve(),
};
function catalogCommand(command, args = []) {
  const text = invoke(command, args);
  try { return JSON.parse(text); } catch { throw new Error(`Owned report catalog ${command} returned invalid data`); }
}
function receipt(value) {
  if (typeof value === 'number' && (!Number.isSafeInteger(value) || value <= 0)) throw new TypeError('Positive owned receipt required');
  if (!['number', 'string'].includes(typeof value) || !/^[1-9][0-9]{0,18}$/.test(String(value)) || BigInt(value) > 9223372036854775807n) {
    throw new TypeError('Positive owned receipt required');
  }
  return String(value);
}
export async function withReportCatalog(callback) {
  if (typeof callback !== 'function') throw new TypeError('Report catalog callback required');
  if (catalogState.context.getStore()) throw new Error('Nested report catalog scopes are not allowed');
  if (process.env.VISUAL_FIXTURE_SERVER === '1') throw new Error('Report catalog requires the real backend, not VISUAL_FIXTURE_SERVER');
  const filename = process.env[manifestVariable];
  if (!filename) throw new Error('Owned deletion quota runner manifest is missing');
  const manifest = readManifest(filename);
  const previous = catalogState.queue;
  let release;
  catalogState.queue = new Promise(resolve => { release = resolve; });
  await previous;
  try {
    return await catalogState.context.run(true, async () => {
      let value;
      let open = true;
      const failures = [];
      const requireOpen = () => { if (!open) throw new Error('Report catalog scope has ended'); };
      try {
        const enabled = catalogCommand('catalog-enable');
        if (!Number.isSafeInteger(enabled.revision) || enabled.revision < 1 || enabled.revision > 64
            || enabled.ruleId !== 9001 || enabled.illegalId !== 31) throw new Error('Invalid synthetic report catalog');
        const catalog = Object.freeze({
          revision: enabled.revision, ruleId: 9001, illegalId: 31, marker: manifest.marker,
          async disable() { requireOpen(); catalogCommand('catalog-disable'); },
          async inspect({ op, target }) {
            requireOpen();
            const result = catalogCommand('catalog-inspect', [receipt(op), receipt(target)]);
            if (!Number.isSafeInteger(result.reportCount) || result.reportCount < 0 || result.reportCount > 32
                || !Array.isArray(result.categories) || result.categories.length !== result.reportCount) {
              throw new Error('Invalid owned report inspection');
            }
            return { reportCount: result.reportCount, categories: result.categories.map(({ revision, id, kind, baseWeight, title }) => ({ revision, id, kind, baseWeight, title })) };
          },
        });
        value = await callback(catalog);
      } catch (error) { failures.push(error); }
      finally {
        open = false;
        // Even a lost enable acknowledgement may have committed activation.
        // Disable is exact-owned and safe to retry; retain proof if it fails.
        try { catalogCommand('catalog-disable'); } catch (error) { failures.push(error); }
      }
      if (failures.length === 1) throw failures[0];
      if (failures.length > 1) throw new AggregateError(failures, 'Report catalog action and cleanup both failed');
      return value;
    });
  } finally { release(); }
}
