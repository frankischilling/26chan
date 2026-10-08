import test from 'node:test';
import assert from 'node:assert/strict';
import { captureParsingRange, sourceParsingRange, settleInitialParsing } from '../../apps/public/client/native-source-events.js';

function fixture() {
  const document = new EventTarget(), nodes = new Map(), events = [];
  document.createEvent = () => ({ initEvent(name, bubbles, cancelable) {
    this.type = name; this.bubbles = bubbles; this.cancelable = cancelable;
  } });
  document.dispatchEvent = event => { events.push(event); };
  document.getElementById = id => nodes.get(id);
  const section = { id: 't9007199254740993', ownerDocument: document, isConnected: true, parentNode: {},
    querySelectorAll: selector => { assert.equal(selector, ':scope > .postContainer'); return posts; } };
  const posts = ['9007199254740993', '9007199254740994', '9007199254740995'].map(id => ({
    id: `pc${id}`, isConnected: true, parentNode: section,
  }));
  for (const node of [section, ...posts]) nodes.set(node.id, node);
  return { section, posts, nodes, events };
}

test('source range arithmetic preserves negative offsets and zero/default limits', () => {
  for (const [length, offset, limit, expected] of [
    [8, undefined, undefined, [0, 8]], [8, 0, 0, [0, 8]], [8, -2, undefined, [6, 8]],
    [8, 1, 2, [1, 3]], [8, 1, 0, [1, 8]], [8, -2, 1, [6, 7]],
    [8, -9, undefined, [-1, 8]], [8, 2, -1, [2, 1]], [0, 0, 0, [0, 0]],
  ]) assert.deepEqual(sourceParsingRange(length, offset, limit), { offset: expected[0], limit: expected[1] });
});

test('committed ranges expose exact string IDs and source Event flags', () => {
  const f = fixture(); const range = captureParsingRange(f.section, -1); assert.equal(range.emit(), true);
  assert.equal(range.emit(), false);
  assert.deepEqual(f.events, [{ type: '4chanParsingDone', bubbles: false, cancelable: false,
    initEvent: f.events[0].initEvent, detail: { threadId: '9007199254740993', offset: 2, limit: 3 } }]);
});

test('replaced, reordered, detached and rolled-back posts invalidate captured ranges', () => {
  for (const mutate of [f => { f.section.isConnected = false; }, f => { f.section.parentNode = {}; },
    f => { f.nodes.set(f.section.id, {}); }, f => { f.posts.pop(); }, f => { f.posts.reverse(); },
    f => { f.posts[1].parentNode = {}; }, f => { f.nodes.set(f.posts[1].id, {}); }]) {
    const f = fixture(), range = captureParsingRange(f.section); mutate(f);
    assert.equal(range.emit(), false); assert.equal(f.events.length, 0);
  }
});

test('initial parsing waits for feature settlement and does not replay', async () => {
  const f = fixture(), controller = new AbortController(); let finish;
  const pending = settleInitialParsing({ sections: [f.section], signal: controller.signal, active: () => true,
    settle: () => new Promise(resolve => { finish = resolve; }) });
  assert.equal(f.events.length, 0); finish(true); assert.equal(await pending, true); assert.equal(f.events.length, 1);
  await Promise.resolve(); assert.equal(f.events.length, 1);
});

test('failed, cancelled, disabled and replaced async passes emit nothing', async () => {
  for (const mode of ['reject', 'false', 'abort', 'disabled', 'replace']) {
    const f = fixture(), controller = new AbortController(); let active = true;
    await settleInitialParsing({ sections: [f.section], signal: controller.signal, active: () => active,
      settle: async () => {
        await Promise.resolve();
        if (mode === 'reject') throw Error('feature failed');
        if (mode === 'false') return false;
        if (mode === 'abort') controller.abort();
        if (mode === 'disabled') active = false;
        if (mode === 'replace') f.nodes.set(f.section.id, {});
        return true;
      } });
    assert.equal(f.events.length, 0, mode);
  }
});

test('depager preserves source numeric IDs and exact adjacent unsafe IDs', async () => {
  const { sourceDepagerThreadId } = await import('../../apps/public/client/native-source-events.js');
  assert.equal(sourceDepagerThreadId('100'), 100);
  assert.equal(sourceDepagerThreadId('9007199254740991'), 9007199254740991);
  assert.equal(sourceDepagerThreadId('9007199254740992'), '9007199254740992');
  assert.equal(sourceDepagerThreadId('9007199254740993'), '9007199254740993');
  for (const id of ['100', '9007199254740992', '9007199254740993']) {
    const f = fixture(); f.section.id = `t${id}`; f.nodes.set(f.section.id, f.section);
    captureParsingRange(f.section, undefined, undefined, sourceDepagerThreadId(id)).emit();
    assert.equal(f.events[0].detail.threadId, id === '100' ? 100 : id);
  }
});

test('bootstrap blocks live insertions through deferred receipts and filters, including initial event listeners', async () => {
  const { createParsingBootstrap } = await import('../../apps/public/client/native-source-events.js');
  const f = fixture(), controller = new AbortController(), bootstrap = createParsingBootstrap(() => ({}));
  let receipt, filters, decorated = false;
  const attemptAppend = () => {
    if (!bootstrap.ready()) return false;
    const post = { id: 'pc9007199254740996', isConnected: true, parentNode: f.section };
    f.posts.push(post); f.nodes.set(post.id, post);
    captureParsingRange(f.section, -1).emit(); return true;
  };
  f.section.ownerDocument.dispatchEvent = event => {
    f.events.push(event);
    if (event.detail.offset === 0) { assert.equal(decorated, true); assert.equal(attemptAppend(), false); }
  };
  const pending = bootstrap.run({ sections: [f.section], signal: controller.signal, active: () => true,
    prepare: async () => { await new Promise(resolve => { receipt = resolve; }); decorated = true; },
    settle: () => new Promise(resolve => { filters = resolve; }),
  });
  assert.equal(attemptAppend(), false); assert.equal(f.events.length, 0);
  receipt(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(decorated, true); assert.equal(attemptAppend(), false); assert.equal(f.events.length, 0);
  filters(true); assert.equal(await pending, true);
  assert.equal(attemptAppend(), true);
  assert.deepEqual(f.events.map(event => event.detail), [
    { threadId: '9007199254740993', offset: 0, limit: 3 },
    { threadId: '9007199254740993', offset: 3, limit: 4 },
  ]);
  assert.equal(await bootstrap.run({}), true); assert.equal(f.events.length, 2);
});

test('bootstrap faults and cancellation keep live controllers closed; intentional disablement can later reopen', async () => {
  const { createParsingBootstrap } = await import('../../apps/public/client/native-source-events.js');
  for (const mode of ['prepare-failure', 'filters-failure', 'abort', 'replace', 'disabled']) {
    const f = fixture(), controller = new AbortController(), config = {};
    const bootstrap = createParsingBootstrap(() => config);
    const ready = await bootstrap.run({ sections: [f.section], signal: controller.signal, active: () => config.disableAll !== true,
      prepare: async () => { if (mode === 'prepare-failure') throw Error('receipt failed'); if (mode === 'disabled') config.disableAll = true; },
      settle: async () => {
        if (mode === 'filters-failure') return false;
        if (mode === 'abort') controller.abort();
        if (mode === 'replace') f.nodes.set(f.section.id, {});
        return true;
      },
    });
    assert.equal(ready, mode === 'disabled', mode); assert.equal(f.events.length, 0);
    delete config.disableAll;
    assert.equal(bootstrap.ready(), mode === 'disabled', mode);
  }
});

test('unfinished bootstrap retries after BFcache with generation ownership and never repeats completed events', async () => {
  const { createParsingBootstrap } = await import('../../apps/public/client/native-source-events.js');
  const f = fixture(), bootstrap = createParsingBootstrap(() => ({})); let release;
  const first = new AbortController();
  const stale = bootstrap.run({ sections: [f.section], signal: first.signal, active: () => true,
    prepare: () => new Promise(resolve => { release = resolve; }), settle: async () => true });
  first.abort();
  const second = new AbortController();
  assert.equal(await bootstrap.run({ sections: [f.section], signal: second.signal, active: () => true,
    prepare: async () => true, settle: async () => true }), true);
  assert.equal(bootstrap.ready(), true); assert.equal(f.events.length, 1);
  release(true); assert.equal(await stale, false); assert.equal(bootstrap.ready(), true);
  second.abort(); assert.equal(await bootstrap.run({}), true); assert.equal(f.events.length, 1);
});
