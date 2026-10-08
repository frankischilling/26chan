import test from 'node:test';
import assert from 'node:assert/strict';
import { cooldownSeconds, createQuickReplyCooldown } from '../../apps/public/client/native-quick-reply-cooldown.js';

function fixture({ timestamp = '100000', replySeconds = 3, imageSeconds = 5, denied = false } = {}) {
  let clock = 100000, next = 0, state, submitted = 0;
  const timers = new Map(), writes = [], values = new Map([['4chan-cd-demo', timestamp]]);
  const storage = {
    getItem(key) { if (denied) throw Error('Denied'); return values.get(key); },
    setItem(key, value) { if (denied) throw Error('Denied'); writes.push([key, value]); values.set(key, value); },
  };
  const controller = createQuickReplyCooldown({ board: 'demo', replySeconds, imageSeconds, storage,
    now: () => clock,
    schedule: callback => { const id = ++next; timers.set(id, callback); return id; },
    cancel: id => timers.delete(id),
    changed: value => { state = value; }, expired: () => { submitted++; },
  });
  return { controller, storage, writes, timers, get state() { return state; }, get submitted() { return submitted; },
    time(value) { clock = value; },
    tick(value) { clock = value; const callbacks = [...timers.values()]; timers.clear(); callbacks.forEach(callback => callback()); },
  };
}

test('ordinary advisory rounds upward and rejects expired, future, absent and malformed timestamps', () => {
  assert.equal(cooldownSeconds('100000', '3', 100001), 3);
  assert.equal(cooldownSeconds('100000', '3', 102001), 1);
  assert.equal(cooldownSeconds('100000', '3', 103000), 0);
  assert.equal(cooldownSeconds('100001', '3', 100000), 0);
  for (const stamp of [null, undefined, '', 'NaN', '1e5', '100000x', '-1', '9007199254740992']) {
    assert.equal(cooldownSeconds(stamp, 3, 100000), 0);
  }
  for (const delay of [undefined, null, '', '-1', '1e2', '86401', 'x']) assert.equal(cooldownSeconds('100000', delay, 100000), 0);
  assert.equal(cooldownSeconds('100000', 0, 100000), 0);
});

test('selected incoming media controls the delay and never writes a posting timestamp', () => {
  const f = fixture();
  assert.equal(f.controller.refresh(false), 3); assert.equal(f.state.label, '3s');
  assert.equal(f.controller.refresh(true), 5); assert.equal(f.state.label, '5s');
  f.tick(101001); assert.equal(f.state.label, '4s');
  f.controller.refresh(false); assert.equal(f.state.label, '2s');
  assert.deepEqual(f.writes, []);
});

test('ordinary clicks toggle auto and expiry submits once, with intent cleared before callback', () => {
  const f = fixture(); f.controller.refresh();
  assert.equal(f.controller.toggle(), true); assert.equal(f.state.label, '3s (auto)');
  assert.equal(f.controller.toggle(), false); assert.equal(f.state.label, '3s');
  f.controller.toggle(); f.tick(103000);
  assert.equal(f.submitted, 1); assert.deepEqual(f.state, { seconds: 0, armed: false, label: 'Post' });
  f.tick(104000); f.controller.refresh(); assert.equal(f.submitted, 1);
  assert.deepEqual(f.writes, []);
});

test('disarming and lifecycle stops fence already queued callbacks and preserve future reopen advisory', () => {
  const f = fixture(); f.controller.refresh(); f.controller.toggle();
  f.controller.disarm(); f.tick(103000); assert.equal(f.submitted, 0);
  f.time(100000); f.controller.refresh(); f.controller.toggle();
  const stale = [...f.timers.values()][0]; f.controller.stop(); f.time(104000); stale();
  assert.equal(f.submitted, 0); assert.equal(f.timers.size, 0);
  f.time(101000); f.controller.refresh(); assert.equal(f.state.label, '2s');
});

test('a readiness refresh at expiry preserves the one-shot intent without synchronous submission', () => {
  const f = fixture(); f.controller.refresh(); f.controller.toggle();
  f.time(103000); f.controller.refresh();
  assert.equal(f.submitted, 0); f.tick(103000); assert.equal(f.submitted, 1);
  f.tick(104000); assert.equal(f.submitted, 1);
});

test('only success writes one board timestamp and persistent text draft restarts the reply delay', () => {
  const f = fixture(); f.controller.refresh(true); f.controller.toggle(); f.controller.stop();
  f.time(105000); f.controller.success(); f.controller.refresh(false);
  assert.deepEqual(f.writes, [['4chan-cd-demo', '105000']]);
  assert.equal(f.state.label, '3s'); assert.equal(f.state.armed, false);
});

test('storage events refresh only the same board and storage area, ignoring removal and unrelated entries', () => {
  const f = fixture(); f.controller.refresh(); f.time(101000);
  for (const event of [{ key: null, newValue: null }, { key: '4chan-cd-other', newValue: '101000' },
    { key: '4chan-cd-demo', newValue: '' }, { key: '4chan-cd-demo', newValue: null },
    { key: '4chan-cd-demo', newValue: '101000', storageArea: {} }]) f.controller.storageEvent(event);
  assert.equal(f.state.label, '3s');
  f.controller.storageEvent({ key: '4chan-cd-demo', newValue: '101000', storageArea: f.storage });
  assert.equal(f.state.label, '3s'); f.tick(103000); assert.equal(f.state.label, '1s');
  assert.equal(f.submitted, 0); assert.deepEqual(f.writes, []);
});

test('clock rollback and future storage values cancel armed submission', () => {
  const f = fixture(); f.controller.refresh(); f.controller.toggle(); f.tick(99999);
  assert.equal(f.state.label, 'Post'); assert.equal(f.submitted, 0); assert.equal(f.state.armed, false);
  f.time(100000); f.controller.refresh(); f.controller.toggle();
  f.controller.storageEvent({ key: '4chan-cd-demo', newValue: '200000' });
  f.tick(203000); assert.equal(f.submitted, 0); assert.equal(f.state.label, 'Post');
});

test('unavailable storage leaves posting usable and retains a successful local advisory', () => {
  const f = fixture({ denied: true }); f.controller.refresh(); assert.equal(f.state.label, 'Post');
  f.controller.success(); f.controller.refresh(); assert.equal(f.state.label, '3s');
  f.tick(103000); assert.equal(f.state.label, 'Post'); assert.equal(f.submitted, 0);
});
