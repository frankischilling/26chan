import test from 'node:test';
import assert from 'node:assert/strict';
import { installReplayAllocationGuard } from '../../apps/public/client/native-replay-worker-allocations.js';
// Allocation bookkeeping only, with no claim about native Canvas rendering.
function setup() {
  class Context { createImageData() { return 'created'; } getImageData() { return 'read'; } }
  class Pixels { constructor(...args) { this.args = args; } }
  const scope = { ArrayBuffer, Uint8Array, Uint8ClampedArray, Uint16Array, Uint32Array, ImageData: Pixels,
    OffscreenCanvasRenderingContext2D: Context };
  return { scope, guard: installReplayAllocationGuard(scope) };
}
test('typed array copies charge bytes; existing buffer views do not double charge', () => {
  const { scope, guard } = setup();
  const bytes = new scope.Uint8Array(12);
  const pixels = new scope.Uint8ClampedArray(bytes);
  const view = new scope.Uint32Array(pixels.buffer);
  assert.equal(guard.audit().chargedBytes, 24);
  assert.equal(view.byteLength, 12);
  assert.ok(bytes instanceof Uint8Array);
  assert.throws(() => new scope.Uint8Array([1,2,3]), /Unexpected/);
});
test('native pixel paths charge before performing allocations and reject oversized requests', () => {
  const { scope, guard } = setup();
  new scope.ImageData(16,16);
  const ctx = new scope.OffscreenCanvasRenderingContext2D();
  assert.equal(ctx.createImageData(8,8), 'created');
  assert.equal(ctx.getImageData(0,0,4,4), 'read');
  assert.equal(guard.audit().chargedBytes, (256+64+16)*4);
  assert.throws(() => new scope.ImageData(1025,1), /bounds/);
  assert.throws(() => ctx.getImageData(0,0,1,Infinity), /bounds/);
});
test('cumulative budget cannot be reset or exceeded by repeated allocations', () => {
  const { scope, guard } = setup();
  guard.charge(guard.audit().maximumBytes - 4);
  new scope.Uint32Array(1);
  assert.equal(guard.audit().chargedBytes, guard.audit().maximumBytes);
  assert.throws(() => new scope.Uint8Array(1), /budget/);
  assert.throws(() => guard.charge(-1), /budget/);
  assert.throws(() => { scope.Uint8Array = Uint8Array; });
});
