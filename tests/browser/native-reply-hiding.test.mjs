import test from 'node:test';
import assert from 'node:assert/strict';
import { HIDDEN_REPLY_LIMITS, readHiddenReplies, renewHiddenReplies } from '../../apps/public/client/native-reply-hiding.js';

test('hidden reply IDs stay exact and timestamps remain bounded data', () => {
  const now = 1800000000000;
  const parsed = readHiddenReplies('{"9007199254740992":1799999999999,"9007199254740993":1800000000000}', now);
  assert.equal(parsed.status, 'ok');
  assert.deepEqual([...parsed.entries.keys()], ['9007199254740992', '9007199254740993']);
  assert.equal(readHiddenReplies(null).entries.size, 0);
  for (const raw of ['null', '[]', '{', '{"__proto__":0}', '{"01":0}', '{"0":0}',
    '{"9223372036854775808":0}', '{"1":-1}', '{"1":1.5}', '{"1":"0"}',
    `{"1":${now + 1}}`, ' '.repeat(HIDDEN_REPLY_LIMITS.storage + 1),
    JSON.stringify(Object.fromEntries(Array.from({ length: 513 }, (_, i) => [String(i + 1), 0])))]) {
    assert.equal(readHiddenReplies(raw, now).status, 'invalid');
  }
});

test('visited replies renew before native seven-day pruning and unseen entries expire', () => {
  const now = 1800000000000, edge = now - HIDDEN_REPLY_LIMITS.age;
  const entries = new Map([['1', edge - 1], ['2', edge - 1], ['3', edge], ['4', now]]);
  assert.deepEqual([...renewHiddenReplies(entries, new Set(['1']), now)], [['1', now], ['3', edge], ['4', now]]);
  assert.equal(entries.size, 4);
});
