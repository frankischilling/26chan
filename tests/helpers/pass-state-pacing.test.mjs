import assert from 'node:assert/strict';
import test from 'node:test';
import { passWritePacer } from '../browser/pass-state-pacing.mjs';
test('multiple contexts reserve room for automatic CAPTCHA writes without changing runtime limits', async () => {
  let now = 1000; const waits = [];
  const pacer = passWritePacer({ now: () => now, sleep: async ms => { waits.push(ms); now += ms; } });
  for (const count of [9, 9, 9, 9, 9, 9, 9, 9, 9]) {
    await pacer.beginCase(); for (let i=0; i<count; ++i) pacer.write(); pacer.endCase(); now += 1000;
  }
  assert.deepEqual(waits, [58000, 58000]);
});
test('natural window expiry needs no extra wait and unexpected request storms fail', async () => {
  let now = 0; const pacer = passWritePacer({ now: () => now, sleep: async () => assert.fail('unnecessary wait') });
  await pacer.beginCase(); for (let i=0;i<12;++i) pacer.write(); pacer.endCase();
  now = 61000; await pacer.beginCase(); for (let i=0;i<13;++i) pacer.write();
  assert.throws(() => pacer.endCase(), /reserved write budget/);
});
