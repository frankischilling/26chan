import test from 'node:test';
import assert from 'node:assert/strict';
import { installReplayWorkerBoundary } from '../../apps/public/client/native-replay-worker-boundary.js';
// Metadata/allocation tests only. These mocks make no native pixel claim.
function setup() {
  class Canvas {
    constructor(width, height) { this.w = width; this.h = height; }
    get width() { return this.w; } set width(value) { this.w = value; }
    get height() { return this.h; } set height(value) { this.h = value; }
    getContext() { return {}; }
  }
  const scope = { OffscreenCanvas: Canvas, ImageData: class {} };
  return { scope, boundary: installReplayWorkerBoundary(scope) };
}
test('history canvas clone preserves metadata and dimensions but stays detached', () => {
  const { scope, boundary } = setup();
  const original = scope.document.createElement('canvas');
  original.width = 32; original.height = 24; original.id = 'tegaki-canvas-25';
  original.className = 'tegaki-layer tegaki-hidden'; original.setAttribute('data-id', 25);
  original.style.opacity = .5; boundary.containers.layers.appendChild(original);
  const clone = original.cloneNode(false);
  assert.equal(clone.width, 32); assert.equal(clone.height, 24);
  assert.equal(clone.id, original.id); assert.equal(clone.className, original.className);
  assert.equal(clone.getAttribute('data-id'), '25'); assert.equal(clone.style.opacity, .5);
  assert.equal(clone.parentNode, null);
  assert.equal(scope.document.getElementById(original.id), original);
  boundary.containers.layers.removeChild(original);
  assert.equal(scope.document.getElementById(original.id), null);
  assert.throws(() => clone.cloneNode(true));
});
test('canvas bounds and cumulative allocations reject before oversized native resize', () => {
  const { scope } = setup();
  const canvas = scope.document.createElement('canvas');
  for (const value of [0,-1,1025,NaN,Infinity,1.5]) assert.throws(() => { canvas.width = value; });
  assert.equal(canvas.width, 1);
  canvas.width = 1024; canvas.height = 1024;
  let accepted = 0;
  assert.throws(() => { for (; accepted < 20; accepted++) canvas.width = 1024; }, /allocation budget/);
  assert.ok(accepted < 16);
});
test('closed worker document refuses unexpected element and metadata paths', () => {
  const { scope } = setup();
  assert.throws(() => scope.document.createElement('script'));
  assert.throws(() => scope.document.getElementById('unknown'));
  const canvas = scope.document.createElement('canvas');
  assert.throws(() => canvas.setAttribute('data-id', 256));
  assert.throws(() => { canvas.id = 'foreign'; });
  assert.throws(() => canvas.toDataURL());
});
