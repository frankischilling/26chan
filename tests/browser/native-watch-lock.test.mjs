import assert from 'node:assert/strict';
import test from 'node:test';
import { NativeWatchLock } from '../../apps/public/client/native-watch-lock.js';

function fixture() {
  const queued = [], timers = new Map(), warnings = []; let serial = 0;
  const lock = new NativeWatchLock({
    acquire: (enter, signal) => { queued.push({ enter, signal }); return new Promise(() => {}); },
    later: (fn, ms) => { assert.equal(ms, 5000); timers.set(++serial, fn); return serial; },
    clear: id => timers.delete(id), warn: text => warnings.push(text),
  });
  return { lock, queued, timers, warnings, expire: () => { for (const fn of [...timers.values()]) fn(); } };
}

test('a hung lock provider settles at its deadline and its late callback cannot mutate state', async () => {
  const f = fixture(); let writes = 0;
  const pending = f.lock.run(() => ++writes); f.expire();
  assert.equal(await pending, false); assert.equal(f.queued[0].signal.aborted, true);
  assert.equal(f.queued[0].enter(), false); assert.equal(writes, 0);
  assert.equal(f.timers.size, 0); assert.deepEqual(f.warnings, ['Watch storage is busy. Try again.']);
  const fresh = f.lock.run(() => ++writes); f.queued[1].enter();
  assert.equal(await fresh, 1); assert.equal(writes, 1); assert.equal(f.timers.size, 0);
});

test('caller cancellation and page suspension revoke waiting actions, including callbacks released after resumption', async () => {
  const f = fixture(), controller = new AbortController(); let writes = 0;
  const first = f.lock.run(() => ++writes, controller.signal); controller.abort();
  assert.equal(await first, false); assert.equal(f.queued[0].enter(), false);
  const second = f.lock.run(() => ++writes); f.lock.suspend();
  assert.equal(await second, false); assert.equal(await f.lock.run(() => ++writes), false);
  f.lock.resume(); assert.equal(f.queued[1].enter(), false);
  const third = f.lock.run(() => ++writes); f.queued[2].enter(); assert.equal(await third, 1);
  assert.equal(writes, 1); assert.equal(f.timers.size, 0); assert.deepEqual(f.warnings, []);
});

test('the pending-action ceiling rejects excess work and recovers every slot after expiry', async () => {
  const f = fixture(); let writes = 0;
  const attempts = Array.from({ length: 32 }, () => f.lock.run(() => ++writes));
  assert.equal(f.queued.length, 16); assert.equal(f.timers.size, 16);
  f.expire(); assert.deepEqual(await Promise.all(attempts), Array(32).fill(false));
  for (const { enter } of f.queued) assert.equal(enter(), false);
  assert.equal(writes, 0); assert.equal(f.timers.size, 0);
  const fresh = f.lock.run(() => ++writes); f.queued.at(-1).enter(); assert.equal(await fresh, 1);
});

test('volatile actions retain their result and acquisition failure does not run the mutation', async () => {
  assert.deepEqual(await new NativeWatchLock().run(() => ({ persisted: false })), { persisted: false });
  for (const acquire of [() => { throw new Error('unavailable'); }, () => Promise.reject(new Error('unavailable'))]) {
    let writes = 0; const messages = [];
    const lock = new NativeWatchLock({ acquire, warn: text => messages.push(text) });
    assert.equal(await lock.run(() => ++writes), false); assert.equal(writes, 0); assert.equal(messages.length, 1);
  }
});
