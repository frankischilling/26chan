import test from 'node:test';
import assert from 'node:assert/strict';
import { createStaffAuthBudget, completeStaffAuthStart, STAFF_AUTH_MAX_WAIT_MS } from './helpers/staff-auth-budget.mjs';

function fixture() {
  let time = 0;
  const sleeps = [];
  const budget = createStaffAuthBudget({
    now: () => time,
    sleep: async ms => { sleeps.push(ms); time += ms; },
  });
  return { budget, sleeps, now: () => time, advance: ms => { time += ms; } };
}

// The callbacks represent browser starts and APIRequestContext/fetch starts.
// The scheduler sees the same completion boundary for either transport.
test('browser and direct API starts share exactly thirty admissions', async () => {
  const f = fixture(), sent = [];
  const browserStart = () => { sent.push(['browser', f.now()]); return 200; };
  const apiStart = () => { sent.push(['api', f.now()]); return 401; };
  for (let index = 0; index < 30; index++) {
    assert.equal(await f.budget.run(index % 2 ? apiStart : browserStart), index % 2 ? 401 : 200);
  }
  assert.deepEqual(f.sleeps, []);
  assert.equal(await f.budget.run(browserStart), 200);
  assert.deepEqual(f.sleeps, [60_100]);
  assert.deepEqual(sent.at(-1), ['browser', 60_100]);
  assert.equal(sent.length, 31);
});

test('the full suite admission sequence waits once, only for the remaining window', async () => {
  const f = fixture();
  for (const count of [7, 5, 18]) {
    for (let index = 0; index < count; index++) await f.budget.run(() => 200);
    f.advance(10_000);
  }
  await f.budget.run(() => 200); // replacement enrollment
  await f.budget.run(() => 200); // replacement login
  assert.deepEqual(f.sleeps, [30_100]);
});

test('completed requests have expired without sleeping after a quiet window', async () => {
  const f = fixture();
  for (let index = 0; index < 30; index++) await f.budget.run(() => 200);
  f.advance(61_000);
  await f.budget.run(() => 200);
  assert.deepEqual(f.sleeps, []);
});

test('completion timestamps include transport delay and failed actions', async () => {
  const f = fixture();
  await assert.rejects(f.budget.run(() => { f.advance(2_000); throw new Error('synthetic failure'); }), /synthetic failure/);
  for (let index = 0; index < 29; index++) await f.budget.run(() => 200);
  f.advance(3_000);
  await f.budget.run(() => 200);
  assert.deepEqual(f.sleeps, [57_100]);
  assert.equal(f.now(), 62_100);
});

test('rejected responses are counted and never retried', async () => {
  const f = fixture();
  let calls = 0;
  const response = { status: () => 429 };
  assert.equal(await f.budget.run(() => { calls++; return response; }), response);
  for (let index = 0; index < 29; index++) await f.budget.run(() => 401);
  await f.budget.run(() => 200);
  assert.equal(calls, 1);
  assert.deepEqual(f.sleeps, [60_100]);
});

test('concurrent callers cannot oversubscribe the final admission', async () => {
  const f = fixture(), sent = [];
  await Promise.all(Array.from({ length: 31 }, (_, index) => f.budget.run(() => { sent.push([index, f.now()]); })));
  assert.deepEqual(sent.slice(0, 30).map(row => row[1]), Array(30).fill(0));
  assert.deepEqual(sent[30], [30, 60_100]);
});

test('a queued caller fails before sending if its wait bound is exhausted', async () => {
  const f = fixture();
  let sent = false;
  const first = f.budget.run(() => { f.advance(STAFF_AUTH_MAX_WAIT_MS + 1); });
  const second = f.budget.run(() => { sent = true; });
  await first;
  await assert.rejects(second, /wait exceeded its bound/);
  assert.equal(sent, false);
});

test('an early scheduler wake rechecks the remaining budget', async () => {
  let time = 0;
  const sleeps = [];
  const budget = createStaffAuthBudget({ now: () => time, sleep: async ms => {
    sleeps.push(ms);
    time += sleeps.length === 1 ? ms - 10 : ms;
  } });
  for (let index = 0; index < 31; index++) await budget.run(() => 200);
  assert.deepEqual(sleeps, [60_100, 10]);
});

test('a stalled scheduler fails without sending request thirty-one', async () => {
  let sent = 0;
  const budget = createStaffAuthBudget({ now: () => 0, sleep: async () => {} });
  for (let index = 0; index < 30; index++) await budget.run(() => { sent++; });
  await assert.rejects(budget.run(() => { sent++; }), /scheduler did not advance/);
  assert.equal(sent, 30);
});

test('a scheduler overshoot fails without sending request thirty-one', async () => {
  let time = 0, sent = 0;
  const budget = createStaffAuthBudget({ now: () => time, sleep: async () => { time += STAFF_AUTH_MAX_WAIT_MS + 1; } });
  for (let index = 0; index < 30; index++) await budget.run(() => { sent++; });
  await assert.rejects(budget.run(() => { sent++; }), /wait exceeded its bound/);
  assert.equal(sent, 30);
});

test('a backward or invalid clock is rejected', async () => {
  let time = 10;
  const budget = createStaffAuthBudget({ now: () => time });
  await budget.run(() => 200);
  time = 9;
  assert.throws(() => budget.run(() => 200), /monotonic clock/);
  time = NaN;
  assert.throws(() => budget.run(() => 200), /monotonic clock/);
});

test('uneven suite timings spend at most one window waiting across recovery starts', async () => {
  const f = fixture();
  for (let index = 0; index < 30; index++) {
    await f.budget.run(() => { f.advance(100); return 200; });
    f.advance(index < 12 ? 300 : 700);
  }
  await f.budget.run(() => { f.advance(100); return 200; });
  await f.budget.run(() => 200);
  assert.ok(f.sleeps.length > 0);
  assert.ok(f.sleeps.reduce((sum, wait) => sum + wait, 0) <= STAFF_AUTH_MAX_WAIT_MS);
});

test('several windows of uneven completions never admit over thirty starts in sixty seconds', async () => {
  const f = fixture(), admissions = [];
  for (let index = 0; index < 125; index++) {
    await f.budget.run(() => { admissions.push(f.now()); f.advance((index % 7) * 11); });
    f.advance((index % 11) * 19);
  }
  for (const time of admissions) {
    assert.ok(admissions.filter(value => value <= time && value > time - 60_000).length <= 30);
  }
  assert.ok(f.sleeps.every(wait => wait <= STAFF_AUTH_MAX_WAIT_MS));
});

for (const asynchronous of [false, true]) {
  test(`${asynchronous ? 'asynchronous' : 'synchronous'} action failure owns the later response rejection`, async () => {
    let rejectResponse;
    const response = new Promise((resolve, reject) => { rejectResponse = reject; });
    const action = asynchronous
      ? async () => { throw new Error('synthetic action failure'); }
      : () => { throw new Error('synthetic action failure'); };
    await assert.rejects(completeStaffAuthStart(response, action), /synthetic action failure/);
    rejectResponse(new Error('synthetic response timeout'));
    // node:test fails this case if the rejected watcher was left unhandled.
    await new Promise(resolve => setImmediate(resolve));
  });
}

test('start-response ownership retains the original response and invokes the action once', async () => {
  const response = { status: () => 200 };
  let calls = 0;
  assert.equal(await completeStaffAuthStart(Promise.resolve(response), () => { calls++; }), response);
  assert.equal(calls, 1);
});
