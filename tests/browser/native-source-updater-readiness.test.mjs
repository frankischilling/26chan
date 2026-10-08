import test from 'node:test';
import assert from 'node:assert/strict';
import { mountNativeThreadUpdater } from '../../apps/public/client/native-thread-updater.js';

function harness(config) {
  let ready = false, clock = 0, timerId = 0, requests = 0;
  const timers = new Map(), saved = new Map();
  const section = { isConnected: true, dataset: {}, querySelectorAll: () => [{ id: 'pc100' }] };
  const document = Object.assign(new EventTarget(), { hidden: false, title: 'Owned',
    getElementById: id => id === 't100' ? section : null, querySelector: () => null, querySelectorAll: () => [],
  });
  const window = new EventTarget();
  const globals = { document, window, sessionStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    setTimeout: (fn, ms) => { const id = ++timerId; timers.set(id, { fn, at: clock + ms }); return id; },
    clearTimeout: id => timers.delete(id),
  };
  for (const [key, value] of Object.entries(globals)) { saved.set(key, Object.getOwnPropertyDescriptor(globalThis, key)); Object.defineProperty(globalThis, key, { configurable: true, writable: true, value }); }
  const updater = mountNativeThreadUpdater({ board: 'demo', thread: '100', settings: () => config, ready: () => ready,
    createTransport: () => ({ refresh: async () => { requests++; return { status: 'not-modified' }; }, cancel() {}, invalidate() {} }),
  });
  return { updater, window, timers, requests: () => requests, open() { ready = true; updater.sync(); },
    async advance(ms) {
      const until = clock + ms;
      while (true) {
        const next = [...timers].filter(([, timer]) => timer.at <= until).sort((a, b) => a[1].at - b[1].at)[0];
        if (!next) break;
        clock = next[1].at; timers.delete(next[0]); next[1].fn();
        await Promise.resolve(); await Promise.resolve();
      }
      clock = until;
    },
    restore() { for (const [key, descriptor] of saved) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else delete globalThis[key]; } },
  };
}

test('actual updater defers auto countdown across a slow bootstrap and restarts after BFcache', async () => {
  const h = harness({ alwaysAutoUpdate: true });
  try {
    await h.advance(30000); assert.equal(h.requests(), 0); assert.equal(h.timers.size, 0);
    h.open(); assert.equal(h.timers.size, 1);
    await h.advance(10000); assert.equal(h.requests(), 1); assert.equal(h.timers.size, 1);
    h.window.dispatchEvent(new Event('pagehide'));
    await h.advance(30000); assert.equal(h.requests(), 1);
    const show = new Event('pageshow'); show.persisted = true; h.window.dispatchEvent(show);
    await h.advance(10000); assert.equal(h.requests(), 2); assert.equal(h.timers.size, 1);
  } finally { h.restore(); }
});

test('actual updater preserves a posted refresh until readiness opens', async () => {
  const h = harness({});
  try {
    h.updater.posted('101', Promise.resolve(true)); await Promise.resolve(); await Promise.resolve();
    await h.advance(30000); assert.equal(h.requests(), 0);
    h.open(); await h.advance(500); assert.equal(h.requests(), 1);
    await h.advance(30000); assert.equal(h.requests(), 1);
  } finally { h.restore(); }
});
