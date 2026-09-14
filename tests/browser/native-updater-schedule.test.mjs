import assert from 'node:assert/strict';
import test from 'node:test';
import { NativeUpdaterSchedule, UPDATE_DELAYS } from '../../apps/public/client/native-updater-schedule.js';

function fixture() {
  let id = 0, polls = 0, hidden = false;
  const timers = new Map(), ticks = [];
  const schedule = new NativeUpdaterSchedule({
    poll: () => { polls++; assert.equal(schedule.begin(), true); },
    tick: value => ticks.push(value), hidden: () => hidden,
    later: (fn, ms) => { assert.equal(ms, 1000); timers.set(++id, fn); return id; },
    clear: id => timers.delete(id),
  });
  const second = () => {
    assert.equal(timers.size, 1);
    const [id, fn] = timers.entries().next().value; timers.delete(id); fn();
    assert.ok(timers.size <= 1);
  };
  return { schedule, timers, ticks, second, polls: () => polls, hide: value => { hidden = value; } };
}

test('Auto starts at ten seconds, runs one request and backs off through the pinned five-minute ceiling', () => {
  const f = fixture(); f.schedule.start();
  for (const delay of [...UPDATE_DELAYS, 300, 300]) {
    assert.equal(f.ticks.at(-1), delay);
    for (let i = 0; i < delay; i++) f.second();
    assert.equal(f.timers.size, 0);
    assert.equal(f.schedule.busy, true);
    f.schedule.finish(0, false);
  }
  assert.equal(f.polls(), 12);
});

test('a forced empty update retains backoff while new posts reset it for visible and hidden tabs', () => {
  const f = fixture(); f.schedule.start();
  for (let i = 0; i < 8; i++) { f.schedule.begin(); f.schedule.finish(0, false); }
  assert.equal(f.ticks.at(-1), 240);
  f.schedule.begin(); f.schedule.finish(0, true); assert.equal(f.ticks.at(-1), 240);
  f.schedule.begin(); f.schedule.finish(2, false); assert.equal(f.ticks.at(-1), 10);
  f.hide(true); f.schedule.begin(); f.schedule.finish(2, false); assert.equal(f.ticks.at(-1), 60);
});

test('visibility resets the countdown but cannot overlap an in-flight request', () => {
  const f = fixture(); f.schedule.start(); f.second(); f.second();
  f.hide(true); f.schedule.visibility(); assert.equal(f.ticks.at(-1), 10);
  assert.equal(f.schedule.delay, 4); assert.equal(f.timers.size, 1);
  f.schedule.begin();
  for (let i = 0; i < 100; i++) { f.hide(i % 2 === 0); f.schedule.visibility(); }
  assert.equal(f.timers.size, 0); assert.equal(f.schedule.begin(), false);
  f.schedule.finish(0, false); assert.equal(f.timers.size, 1);
  assert.equal(f.ticks.at(-1), 15); assert.equal(f.polls(), 0);
});

test('stopped or replaced timer callbacks cannot restart polling and suspension permits one fresh start', () => {
  const f = fixture(); f.schedule.start();
  const stale = f.timers.values().next().value;
  assert.equal(f.schedule.start(), false);
  f.schedule.visibility(); stale(); assert.equal(f.timers.size, 1);
  f.schedule.stop(); stale(); assert.equal(f.timers.size, 0);
  f.schedule.start(); f.schedule.begin(); f.schedule.stop(); f.schedule.finish(0, false);
  assert.equal(f.timers.size, 0);
  f.schedule.start(); f.schedule.begin(); f.schedule.suspend();
  f.schedule.finish(5, false); assert.equal(f.timers.size, 0);
  assert.equal(f.schedule.start(), true); assert.equal(f.ticks.at(-1), 10);
  assert.equal(f.polls(), 0);
});
