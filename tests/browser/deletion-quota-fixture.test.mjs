import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';

const moduleUrl = pathToFileURL(path.resolve('tests/browser/helpers/deletion-quota-fixture.js')).href;

function fixture(source, environment = {}) {
  const directory = mkdtempSync(path.join(tmpdir(), 'quota-helper-unit-'));
  const examples = path.join(directory, 'debug/examples');
  const log = path.join(directory, 'commands');
  mkdirSync(examples, { recursive: true });
  // This stand-in verifies only orchestration. Rust tests qualify database ownership.
  const executable = path.join(examples, 'deletion-quota-fixture');
  writeFileSync(executable, `#!${process.execPath}\nconst fs = require('node:fs');\nfs.appendFileSync(${JSON.stringify(log)}, process.argv[2] + '\\n');\nif (fs.existsSync(${JSON.stringify(directory)} + '/fail-' + process.argv[2])) process.exit(1);\n`);
  chmodSync(executable, 0o700);
  try {
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
      import assert from 'node:assert/strict';
      import { readFileSync, existsSync, chmodSync, writeFileSync, rmSync } from 'node:fs';
      import { prepareDeletionQuotaRun, initializeDeletionQuotaRun, teardownDeletionQuotaRun, withDeletionQuota } from ${JSON.stringify(moduleUrl)};
      ${source}
    `], {
      encoding: 'utf8', timeout: 10_000,
      env: {
        PATH: process.env.PATH, APP_ENV: 'development', CARGO_TARGET_DIR: directory,
        MIGRATION_DATABASE_URL: 'postgres://board_migrator:synthetic@127.0.0.1/imageboard',
        ...environment,
      },
    });
    assert.equal(result.status, 0, result.stderr);
    return { commands: readFileSync(log, { encoding: 'utf8', flag: 'a+' }).trim().split('\n').filter(Boolean), output: result.stdout };
  } finally { rmSync(directory, { recursive: true, force: true }); }
}

test('one exclusive fresh manifest survives repeat config evaluations and retires exactly once', () => {
  const { commands } = fixture(`
    const run = initializeDeletionQuotaRun();
    assert.equal(initializeDeletionQuotaRun(), run);
    assert.notEqual(run.key, process.env.POSTER_ID_KEY);
    const filename = process.env.BROWSER_DELETION_QUOTA_MANIFEST;
    assert.notEqual(filename, '/untrusted/inherited/run.json');
    const manifest = JSON.parse(readFileSync(filename));
    assert.equal(manifest.key, run.key);
    assert.equal(manifest.marker, run.marker);
    teardownDeletionQuotaRun();
    assert.equal(existsSync(filename), false);
    teardownDeletionQuotaRun();
  `, { POSTER_ID_KEY: 'a'.repeat(64), BROWSER_DELETION_QUOTA_MANIFEST: '/untrusted/inherited/run.json' });
  assert.deepEqual(commands, ['init', 'finish']);
});

test('concurrent groups serialize and a failed callback does not strand the next group', () => {
  const { commands } = fixture(`
    initializeDeletionQuotaRun();
    try {
      const sequence = [];
      await Promise.all([
        withDeletionQuota(async () => { sequence.push('first'); await new Promise(r => setTimeout(r, 20)); sequence.push('last'); }),
        withDeletionQuota(async () => { sequence.push('second'); }),
      ]);
      assert.deepEqual(sequence, ['first', 'last', 'second']);
      await assert.rejects(withDeletionQuota(async () => { throw new Error('owned failure'); }), /owned failure/);
      await withDeletionQuota(async () => {});
    } finally { teardownDeletionQuotaRun(); }
  `);
  assert.deepEqual(commands, ['init', 'reset', 'check', 'reset', 'check', 'reset', 'check', 'reset', 'check', 'finish']);
});

test('nested groups fail before any nested reset', () => {
  const { commands } = fixture(`
    initializeDeletionQuotaRun();
    try { await withDeletionQuota(async () => {
      await assert.rejects(withDeletionQuota(async () => {}), /Nested deletion quota/);
    }); } finally { teardownDeletionQuotaRun(); }
  `);
  assert.deepEqual(commands, ['init', 'reset', 'check', 'finish']);
});

test('rejects production, remote, wrong identity and missing worker manifest without launching authority', () => {
  for (const environment of [
    { APP_ENV: 'production' },
    { MIGRATION_DATABASE_URL: 'postgres://board_migrator:x@remote.example/imageboard' },
    { MIGRATION_DATABASE_URL: 'postgres://board_public:x@127.0.0.1/imageboard' },
    { MIGRATION_DATABASE_URL: 'postgres://board_migrator:x@127.0.0.1/imageboard?options=unsafe' },
    { TEST_WORKER_INDEX: '0' },
  ]) {
    const { commands } = fixture('assert.throws(() => initializeDeletionQuotaRun());', environment);
    assert.deepEqual(commands, []);
  }
});

test('missing and insecure manifests reject groups without resetting quota', () => {
  const { commands } = fixture(`
    await assert.rejects(withDeletionQuota(async () => {}), /manifest is missing/);
    initializeDeletionQuotaRun();
    const filename = process.env.BROWSER_DELETION_QUOTA_MANIFEST;
    chmodSync(filename, 0o644);
    await assert.rejects(withDeletionQuota(async () => {}), /Private owned/);
    chmodSync(filename, 0o600);
    teardownDeletionQuotaRun();
  `);
  assert.deepEqual(commands, ['init', 'finish']);
});

test('visual-only callback does not need a manifest or database', () => {
  const { commands } = fixture(`assert.equal(await withDeletionQuota(async () => 42), 42);`, {
    VISUAL_FIXTURE_SERVER: '1', MIGRATION_DATABASE_URL: '',
  });
  assert.deepEqual(commands, []);
});

test('setup rejects parallel workers and retry overrides', async () => {
  const { default: setup } = await import('./helpers/deletion-quota-setup.js');
  for (const config of [
    { workers: 2, projects: [{ retries: 0 }] },
    { workers: 1, fullyParallel: true, projects: [{ retries: 0 }] },
    { workers: 1, projects: [{ retries: 1 }] },
    { workers: 1, projects: [{ retries: 0, fullyParallel: true }] },
  ]) assert.throws(() => setup(config), /one serial worker and zero retries/);
});

test('a worker reuses the current manifest without creating a second actor or retiring the owner', () => {
  const { commands } = fixture(`
    import { spawnSync } from 'node:child_process';
    const run = initializeDeletionQuotaRun();
    try {
      const worker = spawnSync(process.execPath, ['--input-type=module', '-e', ${JSON.stringify(`
        import assert from 'node:assert/strict';
        import { initializeDeletionQuotaRun, teardownDeletionQuotaRun } from ${JSON.stringify(moduleUrl)};
        const run = initializeDeletionQuotaRun();
        assert.notEqual(run.owner_pid, process.pid);
        assert.throws(() => teardownDeletionQuotaRun(), /Only the owning runner/);
      `)}], { env: { ...process.env, TEST_WORKER_INDEX: '0' }, encoding: 'utf8' });
      assert.equal(worker.status, 0, worker.stderr);
    } finally { teardownDeletionQuotaRun(); }
  `);
  assert.deepEqual(commands, ['init', 'finish']);
});

test('missing prebuilt authority gives a bounded build instruction and removes its manifest', () => {
  const { commands } = fixture(`
    process.env.CARGO_TARGET_DIR += '/missing';
    assert.throws(() => initializeDeletionQuotaRun(), /not built; run cargo build/);
    assert.equal(process.env.BROWSER_DELETION_QUOTA_MANIFEST, undefined);
  `);
  assert.deepEqual(commands, []);
});

test('config preparation is memory-only and keeps the identical fresh key across module evaluations', () => {
  const { commands } = fixture(`
    const prepared = prepareDeletionQuotaRun();
    assert.notEqual(prepared.key, process.env.POSTER_ID_KEY);
    assert.equal(process.env.BROWSER_DELETION_QUOTA_MANIFEST, undefined);
    const reevaluated = await import(${JSON.stringify(`${moduleUrl}?separate-config-evaluation`)});
    assert.equal(reevaluated.prepareDeletionQuotaRun(), prepared);
    const { publicConfig } = await import(${JSON.stringify(pathToFileURL(path.resolve('playwright.public-base.js')).href)});
    const config = publicConfig();
    assert.equal(config.webServer.env.POSTER_ID_KEY, prepared.key);
    assert.equal(config.metadata, undefined);
    for (const key of ['STAFF_TRIPCODE_KEY', 'STAFF_POSTER_ID_KEY', 'STAFF_COUNTRY_DATABASE', 'STAFF_PROXY_SOCKET', 'STAFF_PROXY_UID', 'MEDIA_INTAKE_TOKEN']) {
      assert.equal(config.webServer.env[key], '');
    }
    // Neither listing nor a failed web-server startup reaches global setup.
    assert.equal(process.env.BROWSER_DELETION_QUOTA_MANIFEST, undefined);
    teardownDeletionQuotaRun();
  `, { POSTER_ID_KEY: 'b'.repeat(64), BROWSER_DELETION_QUOTA_MANIFEST: '/untrusted/inherited.json' });
  assert.deepEqual(commands, []);
});

test('validated setup initializes exactly the key prepared for the already-started server', () => {
  const { commands } = fixture(`
    const run = prepareDeletionQuotaRun();
    assert.equal(process.env.BROWSER_DELETION_QUOTA_MANIFEST, undefined);
    const { default: setup } = await import(${JSON.stringify(pathToFileURL(path.resolve('tests/browser/helpers/deletion-quota-setup.js')).href)});
    assert.throws(() => setup({ workers: 2, projects: [{ retries: 0 }] }), /serial worker/);
    assert.equal(process.env.BROWSER_DELETION_QUOTA_MANIFEST, undefined);
    setup({ workers: 1, fullyParallel: false, projects: [{ retries: 0, fullyParallel: false }] });
    try {
      const manifest = JSON.parse(readFileSync(process.env.BROWSER_DELETION_QUOTA_MANIFEST));
      assert.equal(manifest.key, run.key);
      assert.equal(manifest.marker, run.marker);
    } finally { teardownDeletionQuotaRun(); }
  `);
  assert.deepEqual(commands, ['init', 'finish']);
});

test('failed retirement retains exact manifest and path until a confirmed successful retry', () => {
  const { commands } = fixture(`
    initializeDeletionQuotaRun();
    const filename = process.env.BROWSER_DELETION_QUOTA_MANIFEST;
    const before = readFileSync(filename, 'utf8');
    const fail = process.env.CARGO_TARGET_DIR + '/fail-finish';
    writeFileSync(fail, 'synthetic failure');
    assert.throws(() => teardownDeletionQuotaRun(), /finish failed/);
    assert.equal(process.env.BROWSER_DELETION_QUOTA_MANIFEST, filename);
    assert.equal(readFileSync(filename, 'utf8'), before);
    rmSync(fail);
    teardownDeletionQuotaRun();
    assert.equal(existsSync(filename), false);
    assert.equal(process.env.BROWSER_DELETION_QUOTA_MANIFEST, undefined);
  `);
  assert.deepEqual(commands, ['init', 'finish', 'finish']);
});

test('callback and check failures both survive and the serial queue is released', () => {
  const { commands } = fixture(`
    initializeDeletionQuotaRun();
    const fail = process.env.CARGO_TARGET_DIR + '/fail-check';
    writeFileSync(fail, 'synthetic failure');
    try {
      const primary = new Error('primary callback failure');
      await assert.rejects(withDeletionQuota(async () => { throw primary; }), error => {
        assert.ok(error instanceof AggregateError);
        assert.equal(error.errors.length, 2);
        assert.equal(error.errors[0], primary);
        assert.match(error.errors[1].message, /check failed/);
        return true;
      });
      rmSync(fail);
      assert.equal(await withDeletionQuota(async () => 17), 17);
    } finally { teardownDeletionQuotaRun(); }
  `);
  assert.deepEqual(commands, ['init', 'reset', 'check', 'reset', 'check', 'finish']);
});

test('an uncertain init retains its proof, refuses blind re-init, and permits exact retirement', () => {
  const { commands } = fixture(`
    const fail = process.env.CARGO_TARGET_DIR + '/fail-init';
    writeFileSync(fail, 'simulate committed init with lost acknowledgement');
    assert.throws(() => initializeDeletionQuotaRun(), /init failed/);
    const filename = process.env.BROWSER_DELETION_QUOTA_MANIFEST;
    assert.ok(filename);
    const proof = readFileSync(filename, 'utf8');
    assert.throws(() => initializeDeletionQuotaRun(), /initialization outcome is uncertain/);
    assert.equal(process.env.BROWSER_DELETION_QUOTA_MANIFEST, filename);
    assert.equal(readFileSync(filename, 'utf8'), proof);
    rmSync(fail);
    teardownDeletionQuotaRun();
    assert.equal(existsSync(filename), false);
    assert.equal(process.env.BROWSER_DELETION_QUOTA_MANIFEST, undefined);
  `);
  assert.deepEqual(commands, ['init', 'finish']);
});
