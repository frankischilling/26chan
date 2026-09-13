import test from 'node:test';
import assert from 'node:assert/strict';
import { TRACK_LIMITS, readTrackIndex, recordTrackedPost, touchTrackedThread, postReceipts } from '../../apps/public/static/post-tracking.v1.js';

function storage() {
  const values = new Map();
  return { values, getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: key => values.delete(key) };
}
test('post receipts preserve concurrent and large IDs, bound input, and retain native awt', () => {
  const records = postReceipts('board-posted-9007199254740993=9007199254740993.1; board-posted-9007199254740994=9007199254740993.0; 4chan_awt=9007199254740993');
  assert.deepEqual(records.map(row => [row.post, row.track, row.watch]), [
    ['9007199254740993', true, true], ['9007199254740994', true, false], ['9007199254740993', false, true],
  ]);
  for (const raw of ['board-posted-01=1.0', 'board-posted-1=2.0', 'board-posted-1=1.2', 'board-posted-9223372036854775808=1.0', 'x'.repeat(8193)]) assert.equal(postReceipts(raw).length, 0);
  assert.equal(postReceipts(Array.from({ length: 40 }, (_, i) => `board-posted-${i + 1}=1.0`).join(';')).length, 32);
});
test('own-post hints retain exact IDs and expire only indexed entries on the current board', () => {
  const store = storage();
  recordTrackedPost(store, 'demo', '9007199254740993', '9007199254740994', 100);
  recordTrackedPost(store, 'other', '200', '201', 100);
  assert.deepEqual(JSON.parse(store.getItem('4chan-track-demo-9007199254740993')), { '>>9007199254740994': 1 });
  recordTrackedPost(store, 'demo', '9007199254740995', '9007199254740996', 100 + TRACK_LIMITS.ageSeconds);
  assert.equal(store.getItem('4chan-track-demo-9007199254740993'), null);
  assert.notEqual(store.getItem('4chan-track-other-200'), null);
  assert.deepEqual([...readTrackIndex(store.getItem('4chan-track-demo-ts'), 100 + TRACK_LIMITS.ageSeconds).keys()], ['9007199254740995']);
});
test('history bounds evict oldest threads and oldest post IDs without 32-bit timestamps', () => {
  const store = storage();
  for (let id = 1; id <= 129; id++) recordTrackedPost(store, 'demo', String(id), String(id), 2200000000 + id);
  assert.equal(readTrackIndex(store.getItem('4chan-track-demo-ts'), 2200000200).size, 128);
  assert.equal(store.getItem('4chan-track-demo-1'), null);
  for (let id = 1000; id < 1513; id++) recordTrackedPost(store, 'demo', '129', String(id), 2200000200);
  const tracked = JSON.parse(store.getItem('4chan-track-demo-129'));
  assert.equal(Object.keys(tracked).length, 512);
  assert.equal(tracked['>>1000'], undefined);
  assert.equal(tracked['>>1512'], 1);
});
test('touching a thread refreshes history age and malformed indices cannot address other keys', () => {
  const store = storage();
  store.setItem('4chan-track-demo-ts', '{"../other":1,"__proto__":1,"01":1,"100":999999999999999}');
  store.setItem('unrelated', 'keep');
  recordTrackedPost(store, 'demo', '100', '101', 100);
  touchTrackedThread(store, 'demo', '100', 200);
  assert.equal(JSON.parse(store.getItem('4chan-track-demo-ts'))['100'], 200);
  assert.equal(store.getItem('unrelated'), 'keep');
  assert.throws(() => recordTrackedPost(store, '../staff', '100', '101', 200));
  assert.throws(() => recordTrackedPost(store, 'demo', '100', '99', 200));
});

test('revisiting an expired tracked thread retains all its own posts while pruning other expired history', () => {
  const store = storage();
  recordTrackedPost(store, 'demo', '100', '101', 100);
  recordTrackedPost(store, 'demo', '100', '102', 100);
  recordTrackedPost(store, 'demo', '200', '201', 100);
  const now = 100 + TRACK_LIMITS.ageSeconds;
  touchTrackedThread(store, 'demo', '100', now);
  assert.deepEqual(JSON.parse(store.getItem('4chan-track-demo-100')), { '>>101': 1, '>>102': 1 });
  assert.equal(store.getItem('4chan-track-demo-200'), null);
  assert.deepEqual(JSON.parse(store.getItem('4chan-track-demo-ts')), { 100: now });
});

test('posting after a long idle period retains existing own-post hints in that thread', () => {
  const store = storage();
  recordTrackedPost(store, 'demo', '100', '101', 100);
  recordTrackedPost(store, 'demo', '100', '102', 100);
  recordTrackedPost(store, 'demo', '200', '201', 100);
  const now = 100 + TRACK_LIMITS.ageSeconds + 1;
  recordTrackedPost(store, 'demo', '100', '103', now);
  assert.deepEqual(JSON.parse(store.getItem('4chan-track-demo-100')), { '>>101': 1, '>>102': 1, '>>103': 1 });
  assert.equal(store.getItem('4chan-track-demo-200'), null);
  assert.deepEqual(JSON.parse(store.getItem('4chan-track-demo-ts')), { 100: now });
});
