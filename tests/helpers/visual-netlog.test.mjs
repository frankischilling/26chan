import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, stat, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { visualNetlogEnabled, visualNetlogPlan, acceptableNetlogSize, prepareVisualNetlog, acceptVisualNetlog, NETLOG_ARTIFACT_BYTES, NETLOG_FILES_PER_SHARD } from './visual-netlog.js';

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
