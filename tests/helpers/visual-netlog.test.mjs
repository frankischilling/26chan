import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, readdir, mkdir, symlink, link, stat, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { visualNetlogEnabled, visualNetlogPlan, acceptableNetlogSize, prepareVisualNetlog, acceptVisualNetlog, finishVisualNetlog, trackVisualNetlogFailure, NETLOG_ARTIFACT_BYTES, NETLOG_FILES_PER_SHARD } from './visual-netlog.js';

test('capture requires all three exact guards', () => {
  const yes = { VISUAL_FIXTURE_SERVER: '1', WINDOWS_VISUAL_NETLOG: '1' };
  assert.equal(visualNetlogEnabled('win32', yes), true);
  for (const platform of ['linux', 'darwin', 'windows']) assert.equal(visualNetlogEnabled(platform, yes), false);
  for (const key of Object.keys(yes)) for (const value of [undefined, '', 'true', '0', '01']) {
    assert.equal(visualNetlogEnabled('win32', { ...yes, [key]: value }), false);
  }
});

test('plans preserve options and args, use absolute isolated paths, and never collide', () => {
  const options = { args: ['--lang=en-US'], headless: false, slowMo: 7, env: { OWNED: 'fixture' } };
  const plan = visualNetlogPlan(options, 'test-results/windows-themes-2', 2, 'owned-one');
  assert.deepEqual(options.args, ['--lang=en-US']);
  assert.deepEqual({ ...plan.options, args: options.args }, options);
  assert.equal(path.isAbsolute(plan.pending), true);
  assert.equal(path.basename(plan.accepted), 'worker-2-owned-one.json');
  assert.equal(path.basename(path.dirname(plan.pending)), 'netlogs-pending');
  assert.equal(path.basename(path.dirname(plan.accepted)), 'netlogs');
  assert.deepEqual(plan.options.args.slice(1), [`--log-net-log=${plan.pending}`, '--net-log-capture-mode=Default', '--net-log-max-size-mb=8']);
  assert.notEqual(visualNetlogPlan({}, 'output', 1).pending, visualNetlogPlan({}, 'output', 1).pending);
  assert.notEqual(visualNetlogPlan({}, 'output', 1, 'same').pending, visualNetlogPlan({}, 'output', 2, 'same').pending);
  for (const arg of ['--log-net-log=old', '--net-log-capture-mode=Everything', '--net-log-max-size-mb=100']) assert.equal(visualNetlogPlan({ args: [arg] }, 'output', 1), null);
  assert.throws(() => visualNetlogPlan({}, 'output', -1));
  assert.throws(() => visualNetlogPlan({}, 'output', 1, '../escape'));
});

test('hard artifact size boundary rejects empty, invalid and oversized files', () => {
  assert.equal(acceptableNetlogSize(1), true);
  assert.equal(acceptableNetlogSize(NETLOG_ARTIFACT_BYTES), true);
  for (const size of [0, -1, NaN, Infinity, 1.5, NETLOG_ARTIFACT_BYTES + 1]) assert.equal(acceptableNetlogSize(size), false);
});

test('only complete bounded captures enter artifact directory, with four-file shard cap', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'visual-netlog-test-'));
  try {
    for (let i = 0; i < NETLOG_FILES_PER_SHARD + 1; i++) {
      const plan = visualNetlogPlan({}, dir, i, 'owned');
      await prepareVisualNetlog(plan);
      await writeFile(plan.pending, JSON.stringify({ constants: {}, events: [] }));
      assert.equal(await acceptVisualNetlog(plan), i < NETLOG_FILES_PER_SHARD ? 'accepted' : 'file-limit');
      if (i < NETLOG_FILES_PER_SHARD) assert.ok((await stat(plan.accepted)).isFile());
      else await assert.rejects(stat(plan.accepted), { code: 'ENOENT' });
    }
    const bad = visualNetlogPlan({}, dir, 10, 'bad');
    await writeFile(bad.pending, '{');
    await assert.rejects(acceptVisualNetlog(bad), SyntaxError);
    await assert.rejects(stat(bad.accepted), { code: 'ENOENT' });
    await writeFile(bad.pending, '{}');
    assert.equal(await acceptVisualNetlog(bad), 'incomplete');
    await writeFile(bad.pending, Buffer.alloc(NETLOG_ARTIFACT_BYTES + 1));
    assert.equal(await acceptVisualNetlog(bad), 'size-limit');
  } finally { await rm(dir, { recursive: true, force: true }); }
});


test('failure tracking precedes worker teardown and preserves original setup/body errors', async () => {
  for (const phase of ['context setup', 'test body', 'test teardown']) {
    const state = { failed: false };
    const info = { status: 'passed', expectedStatus: 'passed' };
    const original = new Error(phase);
    const events = [];
    await assert.rejects(trackVisualNetlogFailure(state, async () => {
      events.push(phase);
      assert.equal(state.failed, false);
      info.status = 'failed';
      throw original;
    }, info), error => error === original);
    events.push('browser close');
    assert.equal(state.failed, true);
    events.push('launchOptions teardown');
    assert.deepEqual(events, [phase, 'browser close', 'launchOptions teardown']);
  }
});

test('only unexpected outcomes mark a worker; failure remains sticky within that worker', async () => {
  for (const [status, expectedStatus, failed] of [
    ['passed', 'passed', false], ['failed', 'failed', false], ['skipped', 'passed', false],
    ['skipped', 'skipped', false], ['failed', 'passed', true], ['timedOut', 'passed', true],
    ['interrupted', 'passed', true], ['passed', 'failed', true],
  ]) {
    const state = { failed: false };
    await trackVisualNetlogFailure(state, async () => {}, { status, expectedStatus });
    assert.equal(state.failed, failed, `${status}/${expectedStatus}`);
  }
  const state = { failed: false };
  await trackVisualNetlogFailure(state, async () => {}, { status: 'failed', expectedStatus: 'passed' });
  await trackVisualNetlogFailure(state, async () => {}, { status: 'passed', expectedStatus: 'passed' });
  assert.equal(state.failed, true);
});

test('successful workers never consume failure slots; four failed captures remain bounded', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'visual-netlog-retention-'));
  try {
    for (let i = 0; i < 36 + NETLOG_FILES_PER_SHARD + 1; i++) {
      const plan = visualNetlogPlan({}, dir, i, 'retention');
      await prepareVisualNetlog(plan);
      await writeFile(plan.pending, JSON.stringify({ constants: {}, events: [] }));
      const failed = i >= 36;
      const result = await finishVisualNetlog(plan, { failed });
      assert.equal(result, !failed ? 'discarded-success' : i < 36 + NETLOG_FILES_PER_SHARD ? 'accepted' : 'file-limit');
      if (!failed) {
        await assert.rejects(stat(plan.pending), { code: 'ENOENT' });
        await assert.rejects(stat(plan.accepted), { code: 'ENOENT' });
      }
    }
    assert.equal((await readdir(path.join(dir, 'netlogs'))).length, NETLOG_FILES_PER_SHARD);
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('successful cleanup uses only its exact owned path and tolerates absent capture', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'visual-netlog-cleanup-'));
  try {
    const plan = visualNetlogPlan({}, dir, 0, 'cleanup');
    const sibling = visualNetlogPlan({}, dir, 1, 'sibling');
    await prepareVisualNetlog(plan);
    await writeFile(plan.pending, 'owned');
    await writeFile(sibling.pending, 'keep');
    const originalPending = plan.pending;
    plan.pending = sibling.pending;
    await assert.rejects(finishVisualNetlog({ pending: sibling.pending }, { failed: false }), /Unowned/);
    assert.equal(await finishVisualNetlog(plan, { failed: false }), 'discarded-success');
    await assert.rejects(stat(originalPending), { code: 'ENOENT' });
    assert.equal(await readFile(sibling.pending, 'utf8'), 'keep');
    assert.equal(await finishVisualNetlog(plan, { failed: false }), 'discarded-success');
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('cleanup refuses symlink/reparse directories', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'visual-netlog-links-'));
  try {
    const target = path.join(dir, 'target');
    const output = path.join(dir, 'output');
    await mkdir(target);
    await mkdir(output);
    await symlink(target, path.join(output, 'netlogs-pending'), 'junction');
    const plan = visualNetlogPlan({}, output, 0, 'linked');
    const file = path.join(target, path.basename(plan.pending));
    await writeFile(file, 'keep');
    assert.equal(await finishVisualNetlog(plan, { failed: false }), 'unsafe-path');
    assert.equal(await readFile(file, 'utf8'), 'keep');
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('failure observer is automatic, registered before context diagnostics, and browser-independent', async () => {
  const source = await readFile(new URL('./visual-diagnostics.js', import.meta.url), 'utf8');
  assert.match(source, /visualNetlogFailure: \[async \(\{ visualNetlogState \}, use, info\) => \{\s*await trackVisualNetlogFailure\(visualNetlogState, use, info\);\s*\}, \{ auto: true \}\]/);
  assert.ok(source.indexOf('visualNetlogFailure:') < source.indexOf('visualDiagnostics:'));
  assert.match(source, /try \{ console.log\(`Synthetic NetLog artifact: \$\{await finishVisualNetlog\(plan, visualNetlogState\)\}\.`\); \}\s*catch/);
});


test('cleanup refuses linked pending files without touching their targets', async t => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'visual-netlog-file-link-'));
  try {
    const plan = visualNetlogPlan({}, dir, 0, 'linked-file');
    await prepareVisualNetlog(plan);
    const target = path.join(dir, 'target.json');
    await writeFile(target, 'keep');
    try { await symlink(target, plan.pending, 'file'); }
    catch (error) {
      if (process.platform === 'win32' && error.code === 'EPERM') {
        t.skip('File symlinks require Windows symlink privileges');
        return;
      }
      throw error;
    }
    assert.equal(await finishVisualNetlog(plan, { failed: false }), 'unsafe-path');
    assert.equal(await readFile(target, 'utf8'), 'keep');
  } finally { await rm(dir, { recursive: true, force: true }); }
});


test('failed admission uses original paths even when the exposed plan is altered', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'visual-netlog-owned-failure-'));
  try {
    const plan = visualNetlogPlan({}, dir, 0, 'owned-failure');
    await prepareVisualNetlog(plan);
    const { pending, accepted } = plan;
    const own = JSON.stringify({ constants: {}, events: [{ owned: true }] });
    await writeFile(pending, own);
    const unrelated = path.join(dir, 'unrelated.json');
    const wrongDestination = path.join(dir, 'wrong-destination.json');
    await writeFile(unrelated, 'invalid JSON: must never be read');
    plan.pending = unrelated;
    plan.accepted = wrongDestination;
    await assert.rejects(finishVisualNetlog({ ...plan }, { failed: true }), /Unowned/);
    assert.equal(await finishVisualNetlog(plan, { failed: true }), 'accepted');
    assert.equal(await readFile(accepted, 'utf8'), own);
    assert.equal(await readFile(unrelated, 'utf8'), 'invalid JSON: must never be read');
    await assert.rejects(stat(wrongDestination), { code: 'ENOENT' });
    await assert.rejects(stat(pending), { code: 'ENOENT' });
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test('failed admission refuses source and artifact symlink/junction directories before reading', async () => {
  for (const linkedDirectory of ['netlogs-pending', 'netlogs']) {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'visual-netlog-admission-link-'));
    try {
      const target = path.join(dir, 'unrelated');
      const output = path.join(dir, 'output');
      await mkdir(target);
      await mkdir(output);
      await symlink(target, path.join(output, linkedDirectory), 'junction');
      const plan = visualNetlogPlan({}, output, 0, 'linked-failure');
      if (linkedDirectory !== 'netlogs-pending') await prepareVisualNetlog(plan);
      await writeFile(plan.pending, 'invalid JSON: must never be read');
      assert.equal(await finishVisualNetlog(plan, { failed: true }), 'unsafe-path');
      assert.equal(await readFile(plan.pending, 'utf8'), 'invalid JSON: must never be read');
      await assert.rejects(stat(plan.accepted), { code: 'ENOENT' });
      if (linkedDirectory === 'netlogs') assert.deepEqual(await readdir(target), []);
    } finally { await rm(dir, { recursive: true, force: true }); }
  }
});

test('failed admission refuses symlinked, hardlinked, and nonregular captures', async t => {
  for (const kind of ['symbolic', 'hard', 'directory']) {
    await t.test(kind, async t => {
      const dir = await mkdtemp(path.join(os.tmpdir(), 'visual-netlog-admission-file-'));
      try {
        const plan = visualNetlogPlan({}, dir, 0, 'linked-source');
        await prepareVisualNetlog(plan);
        const target = path.join(dir, 'unrelated.json');
        await writeFile(target, 'invalid JSON: must never be read');
        if (kind === 'directory') await mkdir(plan.pending);
        else if (kind === 'hard') await link(target, plan.pending);
        else {
          try { await symlink(target, plan.pending, 'file'); }
          catch (error) {
            if (process.platform === 'win32' && error.code === 'EPERM') {
              t.skip('File symlinks require Windows symlink privileges');
              return;
            }
            throw error;
          }
        }
        assert.equal(await finishVisualNetlog(plan, { failed: true }), 'unsafe-path');
        assert.equal(await readFile(target, 'utf8'), 'invalid JSON: must never be read');
        await assert.rejects(stat(plan.accepted), { code: 'ENOENT' });
      } finally { await rm(dir, { recursive: true, force: true }); }
    });
  }
});

test('failed admission never replaces an existing artifact path', async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'visual-netlog-existing-artifact-'));
  try {
    const plan = visualNetlogPlan({}, dir, 0, 'existing');
    await prepareVisualNetlog(plan);
    await mkdir(path.dirname(plan.accepted));
    await writeFile(plan.pending, 'invalid JSON: must never be read');
    await writeFile(plan.accepted, 'keep existing artifact');
    assert.equal(await finishVisualNetlog(plan, { failed: true }), 'existing-path');
    assert.equal(await readFile(plan.accepted, 'utf8'), 'keep existing artifact');
  } finally { await rm(dir, { recursive: true, force: true }); }
});
