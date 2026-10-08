// Test-only access to the exact production constructors. No extracted or
// rewritten constructor/dispatch semantics and no source decoder are executed.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

export const PINNED_REPLAY_SOURCE = Object.freeze({
  bytes: 110619,
  sha256: 'daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744',
  version: '0.9.4',
});

export async function createInertReplayRuntime() {
  const source = await readFile(new URL('../../apps/public/vendor/tegaki/0.9.4/tegaki.min.js', import.meta.url));
  assert.equal(source.byteLength, PINNED_REPLAY_SOURCE.bytes);
  assert.equal(createHash('sha256').update(source).digest('hex'), PINNED_REPLAY_SOURCE.sha256);
  const calls = [];
  const forbidden = name => (...args) => {
    calls.push(name);
    throw new Error(`Unexpected runtime activation: ${name} (${args.length} arguments)`);
  };
  const sandbox = {
    navigator: {},
    // Source bootstrap only captures this reference in $T.docEl.
    document: new Proxy({ documentElement: Object.freeze({}) }, {
      get: (target, key) => key === 'documentElement' ? target[key] : forbidden(`document.${String(key)}`)(),
    }),
    fetch: forbidden('fetch'), requestAnimationFrame: forbidden('requestAnimationFrame'),
    cancelAnimationFrame: forbidden('cancelAnimationFrame'),
    setTimeout: forbidden('setTimeout'), clearTimeout: forbidden('clearTimeout'),
    setInterval: forbidden('setInterval'), clearInterval: forbidden('clearInterval'),
    performance: { now: forbidden('performance.now') },
    Image: forbidden('Image'), ImageData: forbidden('ImageData'),
    Blob: forbidden('Blob'), URL: { createObjectURL: forbidden('createObjectURL') },
    __forbidden: forbidden,
  };
  sandbox.window = sandbox;
  const context = vm.createContext(sandbox, { codeGeneration: { strings: false, wasm: false } });
  vm.runInContext(source.toString('utf8'), context, { timeout: 2000, filename: 'pinned-production-tegaki-0.9.4.js' });
  assert.equal(sandbox.Tegaki.VERSION, PINNED_REPLAY_SOURCE.version);
  // The source class is lexical, not a production export. This literal test
  // setup constructs an inert viewer directly; open/init/play are never called.
  const viewer = vm.runInContext(`(() => {
    const viewer = new TegakiReplayViewer();
    for (const name of Object.getOwnPropertyNames(TegakiReplayViewer.prototype)) {
      if (name !== 'constructor' && name !== 'getEventIdMap') {
        TegakiReplayViewer.prototype[name] = __forbidden('viewer.' + name);
      }
    }
    viewer.onFrameThis = __forbidden('viewer.onFrameThis');
    for (const Constructor of Object.values(viewer.getEventIdMap())) {
      Constructor.prototype.dispatch = __forbidden(Constructor.name + '.dispatch');
      Constructor.prototype.pack = __forbidden(Constructor.name + '.pack');
      Constructor.unpack = __forbidden(Constructor.name + '.unpack');
    }
    for (const [label, object] of Object.entries({ Tegaki, TegakiUI, TegakiHistory, TegakiLayers, TegakiPressure, UZIP })) {
      for (const key of Object.keys(object)) {
        if (typeof object[key] === 'function') object[key] = __forbidden(label + '.' + key);
      }
    }
    return viewer;
  })()`, context, { timeout: 1000, filename: 'inert-replay-test-setup.js' });
  delete sandbox.__forbidden;
  assert.deepEqual(calls, []);
  return { viewer, calls, source: PINNED_REPLAY_SOURCE };
}

// Own-field snapshots only: methods remain on the real source prototypes.
export function ownEventFields(event) { return Object.fromEntries(Object.entries(event)); }
