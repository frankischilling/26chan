import { performance } from 'node:perf_hooks';
import { setTimeout as delay } from 'node:timers/promises';

export const STAFF_AUTH_WINDOW_MS = 60_000;
export const STAFF_AUTH_MAX_WAIT_MS = 61_000;
const limit = 30;
const boundaryMarginMs = 100;

// All staff cases share one server and one worker. Account for every start,
// including rejected credentials, without changing or retrying the server limit.
export function createStaffAuthBudget({ now = () => performance.now(), sleep = delay } = {}) {
  const completed = [];
  let tail = Promise.resolve();
  let previous = -Infinity;
  function clock() {
    const value = now();
    if (!Number.isFinite(value) || value < previous) throw new Error('Staff authentication budget requires a monotonic clock');
    previous = value;
    return value;
  }
  function run(start) {
    const deadline = clock() + STAFF_AUTH_MAX_WAIT_MS;
    const pending = tail.then(async () => {
      for (;;) {
        const current = clock();
        if (current > deadline) throw new Error('Staff authentication budget wait exceeded its bound');
        while (completed.length && current >= completed[0] + STAFF_AUTH_WINDOW_MS + boundaryMarginMs) completed.shift();
        if (completed.length < limit) break;
        const wait = completed[0] + STAFF_AUTH_WINDOW_MS + boundaryMarginMs - current;
        if (current + wait > deadline) throw new Error('Staff authentication budget wait exceeded its bound');
        await sleep(wait);
        if (clock() <= current) throw new Error('Staff authentication budget scheduler did not advance');
      }
      try {
        return await start();
      } finally {
        // Completion is later than server admission, including failed responses.
        // Serializing starts makes this a conservative bound on wire requests.
        completed.push(clock());
      }
    });
    tail = pending.catch(() => {});
    return pending;
  }
  return { run };
}

// Own both promises even when clicking fails before a response can arrive.
export async function completeStaffAuthStart(received, action) {
  return (await Promise.all([received, Promise.resolve().then(action)]))[0];
}
