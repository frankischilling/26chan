import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';

const moduleUrl = pathToFileURL(path.resolve('tests/browser/helpers/poll-fixture.js')).href;
const secret = 'synthetic-credential-and-private-diagnostic';

function fixture(source, { environment = {}, failures = [], receipt = '7', scores = [2, 3, null] } = {}) {
  const directory = mkdtempSync(path.join(tmpdir(), 'poll-helper-unit-'));
  const log = path.join(directory, 'commands');
  const executable = path.join(directory, 'psql');
  // This stand-in tests process guards and cleanup orchestration, not SQL or rendering.
  writeFileSync(executable, `#!${process.execPath}
    const fs = require('node:fs');
    const input = fs.readFileSync(0, 'utf8');
    const command = input.includes('CREATE TEMP TABLE owned_poll_slots') ? 'seed'
      : input.includes('DELETE FROM poll_private.polls') ? 'cleanup' : 'inspect';
    fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify({ command, input, args: process.argv.slice(2) }) + '\\n');
    if (${JSON.stringify(failures)}.includes(command)) {
      console.error(${JSON.stringify(secret)}); process.exit(1);
    }
    if (command === 'seed') console.log(${JSON.stringify(receipt)});
    if (command === 'inspect') console.log(JSON.stringify({ vote_count: 6, scores: ${JSON.stringify(scores)} }));
  `);
  chmodSync(executable, 0o700);
  try {
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
      import assert from 'node:assert/strict';
      import { withOwnedPolls } from ${JSON.stringify(moduleUrl)};
      ${source}
    `], {
      encoding: 'utf8', timeout: 10_000,
      env: { PATH: `${directory}${path.delimiter}${process.env.PATH}`, APP_ENV: 'development',
        MIGRATION_DATABASE_URL: `postgres://board_migrator:${secret}@127.0.0.1/imageboard`, ...environment },
    });
    assert.equal(result.status, 0, result.stderr);
    assert.ok(!`${result.stdout}${result.stderr}`.includes(secret), 'Credentials and private diagnostics must not escape');
    const commands = existsSync(log) ? readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map(line => JSON.parse(line)) : [];
    for (const command of commands) assert.ok(!command.args.join(' ').includes(secret), 'Credentials must not appear in process arguments');
    return commands;
  } finally { rmSync(directory, { recursive: true, force: true }); }
}

function ownedCleanup(commands) {
  const seed = commands.find(command => command.command === 'seed');
  const cleanup = commands.find(command => command.command === 'cleanup');
  assert.ok(cleanup, 'Cleanup must be attempted');
  assert.deepEqual(cleanup.args, seed.args, 'Cleanup must use the exact original ownership values');
  assert.match(cleanup.input, /id=:'first'::bigint AND title=:'title'/);
  assert.match(cleanup.input, /id=:'second'::bigint AND title=:'marker' \|\| ' second'/);
  assert.match(cleanup.input, /id=:'hidden'::bigint AND title=:'marker' \|\| ' private'/);
  assert.match(cleanup.input, /id BETWEEN 1 AND 9 AND title=:'marker' \|\| ' low-ID'/);
  assert.match(cleanup.input, /SET LOCAL lock_timeout='5s'; SET LOCAL statement_timeout='10s'/);
  assert.equal((cleanup.input.match(/DELETE FROM/g) || []).length, 1);
}

test('successful owned fixtures check results, return a single-digit receipt and clean up once', () => {
  const commands = fixture(`
    const result = await withOwnedPolls(async fixture => {
      assert.equal(fixture.lowId, '7');
      assert.ok(BigInt(fixture.first) > BigInt(fixture.second));
      assert.equal(Buffer.byteLength(fixture.title), 512);
      assert.equal(Buffer.byteLength(fixture.description), 16384);
      assert.equal(Buffer.byteLength(fixture.captions[0]), 1024);
      return 'owned result';
    });
    assert.equal(result, 'owned result');
  `);
  assert.deepEqual(commands.map(command => command.command), ['seed', 'inspect', 'cleanup']);
  assert.match(commands[0].input, /generate_series\(1,9\)/);
  assert.doesNotMatch(commands[0].input, /ON CONFLICT/);
  ownedCleanup(commands);
});

test('production, remote, wrong-role and visual backends are rejected before any spawn', () => {
  for (const environment of [
    { APP_ENV: 'production' }, { VISUAL_FIXTURE_SERVER: '1' },
    { MIGRATION_DATABASE_URL: 'postgres://board_migrator:x@remote.example/imageboard' },
    { MIGRATION_DATABASE_URL: 'postgres://board_public:x@127.0.0.1/imageboard' },
    { MIGRATION_DATABASE_URL: 'postgres://board_migrator:x@127.0.0.1/imageboard?options=unsafe' },
    { MIGRATION_DATABASE_URL: 'invalid' },
  ]) {
    const commands = fixture(`
      await assert.rejects(withOwnedPolls(() => { throw new Error('Callback must not run'); }), /Poll fixtures require/);
    `, { environment });
    assert.deepEqual(commands, []);
  }
});

test('a callback failure still attempts exact owned cleanup', () => {
  const commands = fixture(`
    const failure = new Error('Owned callback failure');
    await assert.rejects(withOwnedPolls(() => { throw failure; }), error => error === failure);
  `);
  assert.deepEqual(commands.map(command => command.command), ['seed', 'cleanup']);
  ownedCleanup(commands);
});

test('a failed seed has no callback or retry and still attempts bounded cleanup', () => {
  const commands = fixture(`
    let calls = 0;
    await assert.rejects(withOwnedPolls(() => { calls += 1; }), /Owned poll fixture database command failed/);
    assert.equal(calls, 0);
  `, { failures: ['seed'] });
  assert.deepEqual(commands.map(command => command.command), ['seed', 'cleanup']);
  ownedCleanup(commands);
});

test('callback and cleanup failures are both preserved without leaking database diagnostics', () => {
  const commands = fixture(`
    const failure = new Error('Owned callback failure');
    await assert.rejects(withOwnedPolls(() => { throw failure; }), error => {
      assert.ok(error instanceof AggregateError);
      assert.equal(error.errors.length, 2);
      assert.equal(error.errors[0], failure);
      assert.equal(error.errors[1].message, 'Owned poll fixture database command failed');
      console.log(error.errors.map(value => value.stack).join('\\n'));
      return true;
    });
  `, { failures: ['cleanup'] });
  assert.deepEqual(commands.map(command => command.command), ['seed', 'cleanup']);
  ownedCleanup(commands);
});

test('failed seed and failed cleanup remain separate errors with no retry', () => {
  const commands = fixture(`
    await assert.rejects(withOwnedPolls(() => { throw new Error('Callback must not run'); }), error => {
      assert.ok(error instanceof AggregateError);
      assert.equal(error.errors.length, 2);
      for (const failure of error.errors) assert.equal(failure.message, 'Owned poll fixture database command failed');
      console.log(error.errors.map(value => value.stack).join('\\n'));
      return true;
    });
  `, { failures: ['seed', 'cleanup'] });
  assert.deepEqual(commands.map(command => command.command), ['seed', 'cleanup']);
  ownedCleanup(commands);
});

test('an invalid single-digit receipt fails before browsing and cleans up uncertain seeds', () => {
  const commands = fixture(`
    await assert.rejects(withOwnedPolls(() => { throw new Error('Callback must not run'); }), /single-digit poll fixture receipt is missing/);
  `, { receipt: '10' });
  assert.deepEqual(commands.map(command => command.command), ['seed', 'cleanup']);
  ownedCleanup(commands);
});

test('changed supplied results fail the read-only invariant and still clean up', () => {
  const commands = fixture(`
    await assert.rejects(withOwnedPolls(() => {}), /Browsing must preserve supplied poll results/);
  `, { scores: [2, 4, null] });
  assert.deepEqual(commands.map(command => command.command), ['seed', 'inspect', 'cleanup']);
  ownedCleanup(commands);
});
