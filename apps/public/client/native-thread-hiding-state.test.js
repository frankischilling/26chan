import assert from 'node:assert/strict';
import test from 'node:test';
import {
  HIDDEN_THREAD_LIMITS, threadHidingKeys, readHiddenThreads, changeHiddenThread,
  renewHiddenThreads, planHiddenThreadPurge, completeHiddenThreadPurge,
} from './native-thread-hiding-state.js';

const now = 2000000000000;
const raw = JSON.stringify({ '1': 0, '9007199254740993': 1, '9223372036854775807': now - 1 });
const success = ids => ({ status: 'complete', results: [{ board: 'demo', status: 'ok', posts: ids.map(no => ({ no })) }] });
const plan = () => planHiddenThreadPurge('demo', raw, null, now);
const complete = (cycle, currentRaw = raw, currentPurgeRaw = null, time = now + 1) =>
  completeHiddenThreadPurge(plan(), cycle, currentRaw, currentPurgeRaw, time);

test('thread storage uses board-specific native keys, never reply keys', () => {
  assert.deepEqual(threadHidingKeys('demo'), { hidden: '4chan-hide-t-demo', purge: '4chan-purge-t-demo' });
  assert.notDeepEqual(threadHidingKeys('demo'), threadHidingKeys('test'));
  for (const board of ['', '../demo', 'Demo', 'demo/', 'a'.repeat(11), null, 1]) {
    assert.equal(threadHidingKeys(board), null);
  }
});

test('hidden IDs retain exact signed-i64 precision and old records do not expire by age', () => {
  assert.deepEqual([...readHiddenThreads(raw, now).entries], [
    ['1', 0], ['9007199254740993', 1], ['9223372036854775807', now - 1],
  ]);
  assert.equal(readHiddenThreads(null, now).entries.size, 0);
});

test('malformed or excessive hidden storage is rejected without a replacement record', () => {
  for (const value of ['{', '[]', 'null', 'true', '{"01":1}', '{"0":1}', '{"-1":1}',
    '{"9223372036854775808":1}', '{"1":-1}', '{"1":1.5}', '{"1":"1"}',
    '{"__proto__":1}', JSON.stringify({ '1': now + 1 }), ' '.repeat(HIDDEN_THREAD_LIMITS.storage + 1),
    JSON.stringify(Object.fromEntries(Array.from({ length: 513 }, (_, i) => [String(i + 1), 1])))]) {
    assert.deepEqual(readHiddenThreads(value, now), { status: 'invalid' });
  }
  for (const time of [-1, NaN, Infinity, now + 0.5, Number.MAX_SAFE_INTEGER + 1]) {
    assert.deepEqual(readHiddenThreads(raw, time), { status: 'invalid' });
  }
});

test('explicit hide, re-hide and unhide produce native timestamps and remove an empty key', () => {
  assert.equal(changeHiddenThread(null, '9007199254740993', true, now).raw, '{"9007199254740993":2000000000000}');
  assert.equal(changeHiddenThread('{"1":1}', '1', true, now).entries.get('1'), now);
  assert.equal(changeHiddenThread('{"1":1}', '1', false, now).raw, null);
  assert.equal(changeHiddenThread(null, '1', false, now).raw, null);
  for (const id of ['01', '0', '9223372036854775808', 1, null]) {
    assert.equal(changeHiddenThread(raw, id, true, now).status, 'invalid');
  }
  assert.equal(changeHiddenThread(raw, '1', 'true', now).status, 'invalid');
  assert.equal(changeHiddenThread('{', '1', false, now).status, 'invalid');
});

test('the entry cap permits renewing and removing existing hides but not adding another', () => {
  const full = JSON.stringify(Object.fromEntries(Array.from({ length: 512 }, (_, i) => [String(i + 1), 1])));
  assert.equal(changeHiddenThread(full, '513', true, now).status, 'limit');
  assert.equal(changeHiddenThread(full, '1', true, now).entries.size, 512);
  assert.equal(changeHiddenThread(full, '1', false, now).entries.size, 511);
});

test('visible renewal never inserts unhidden threads or expires unseen hides', () => {
  const updated = renewHiddenThreads(raw, new Set(['1', '2']), now);
  assert.equal(updated.entries.get('1'), now);
  assert.equal(updated.entries.has('2'), false);
  assert.equal(updated.entries.get('9007199254740993'), 1);
  assert.equal(updated.entries.size, 3);
  assert.equal(readHiddenThreads(raw, now).entries.get('1'), 0);
  assert.equal(renewHiddenThreads(raw, ['1'], now).status, 'invalid');
  assert.equal(renewHiddenThreads(raw, new Set(['01']), now).status, 'invalid');
  assert.equal(renewHiddenThreads(raw, new Set([null]), now).status, 'invalid');
});

test('purge planning requires nonempty hides and the native strict twelve-hour interval', () => {
  assert.equal(plan().status, 'due');
  assert.equal(planHiddenThreadPurge('demo', null, null, now).status, 'empty');
  assert.equal(planHiddenThreadPurge('demo', raw, String(now - HIDDEN_THREAD_LIMITS.purgeMs), now).status, 'cooldown');
  assert.equal(planHiddenThreadPurge('demo', raw, String(now - HIDDEN_THREAD_LIMITS.purgeMs - 1), now).status, 'due');
  assert.equal(planHiddenThreadPurge('demo', raw, '0', now).status, 'due');
  assert.equal(planHiddenThreadPurge('demo', raw, String(now), now).status, 'cooldown');
});

test('invalid purge metadata is preserved rather than silently reset', () => {
  for (const stamp of ['', '00', ' 1', '1 ', '1e3', '1.0', '-1', 'NaN', String(now + 1), '9007199254740992', 0]) {
    assert.deepEqual(planHiddenThreadPurge('demo', raw, stamp, now), { status: 'invalid' });
  }
  assert.equal(planHiddenThreadPurge('../demo', raw, null, now).status, 'invalid');
  assert.equal(planHiddenThreadPurge('demo', '{', null, now).status, 'invalid');
});

test('a complete live list retains exact matching IDs with the native value one', () => {
  const result = complete(success(['9007199254740993', '9223372036854775807', '7']));
  assert.equal(result.status, 'ready');
  assert.deepEqual([...result.entries], [['9007199254740993', 1], ['9223372036854775807', 1]]);
  assert.equal(result.raw, '{"9007199254740993":1,"9223372036854775807":1}');
  assert.equal(result.purgeRaw, String(now + 1));
});

test('an authoritative empty live list removes the hidden key and records successful cleanup', () => {
  const result = complete(success([]));
  assert.equal(result.status, 'ready');
  assert.equal(result.raw, null);
  assert.equal(result.entries.size, 0);
  assert.equal(result.purgeRaw, String(now + 1));
});

test('failed or incomplete refreshes never produce a hidden record or purge timestamp', () => {
  for (const status of ['cancelled', 'network-error', 'cycle-timeout', 'cycle-limit', 'busy', 'cooldown']) {
    assert.deepEqual(complete({ ...success([]), status }), { status: 'unavailable' });
  }
  for (const status of ['http-error', 'invalid-catalog', 'invalid-mime', 'response-limit', 'request-timeout']) {
    assert.deepEqual(complete({ status: 'complete', results: [{ board: 'demo', status, posts: [] }] }), { status: 'unavailable' });
  }
  for (const result of [null, {}, { status: 'complete', results: [] },
    { status: 'complete', results: [...success([]).results, ...success([]).results] }]) {
    assert.deepEqual(complete(result), { status: 'unavailable' });
  }
});

test('wrong-board, duplicate, imprecise and excessive live IDs cannot prune saved hides', () => {
  const wrongBoard = success([]); wrongBoard.results[0].board = 'test';
  assert.equal(complete(wrongBoard).status, 'unavailable');
  for (const ids of [['1', '1'], ['01'], ['0'], ['9223372036854775808'], [null], [9007199254740992],
    Array.from({ length: 513 }, (_, i) => String(i + 1))]) {
    assert.deepEqual(complete(success(ids)), { status: 'unavailable' });
  }
  assert.equal(complete({ status: 'complete', results: [{ board: 'demo', status: 'ok', posts: [null] }] }).status, 'unavailable');
});

test('new hides, unhides, renewal, clear-history and other-tab purges invalidate queued cleanup', () => {
  for (const changed of [changeHiddenThread(raw, '2', true, now).raw,
    changeHiddenThread(raw, '1', false, now).raw, renewHiddenThreads(raw, new Set(['1']), now).raw,
    null, '{', ` ${raw}`]) {
    assert.deepEqual(complete(success([]), changed), { status: 'stale' });
  }
  assert.deepEqual(complete(success([]), raw, String(now)), { status: 'stale' });
});

test('invalid plans and backwards clocks cannot authorize cleanup', () => {
  for (const candidate of [null, {}, { status: 'empty' }, { ...plan(), started: NaN }, { ...plan(), board: '../demo' }]) {
    assert.deepEqual(completeHiddenThreadPurge(candidate, success([]), raw, null, now), { status: 'invalid' });
  }
  assert.equal(complete(success([]), raw, null, now - 1).status, 'invalid');
  assert.equal(complete(success([]), raw, null, Infinity).status, 'invalid');
});
