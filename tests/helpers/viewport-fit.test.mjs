import assert from 'node:assert/strict';
import test from 'node:test';
import vm from 'node:vm';
import { assertViewportFit, qualifyOverflowGuard } from '../browser/helpers/viewport-fit.js';

function page({ width = 390, transient = 0, ignoreMinimum = false } = {}) {
  let reads = 0;
  const style = { minWidth: '' };
  const root = { style, get scrollWidth() {
    reads += 1;
    return Math.max(width + (reads <= transient ? 32 : 0), ignoreMinimum ? 0 : Number.parseFloat(style.minWidth) || 0);
  } };
  const context = vm.createContext({ document: { documentElement: root, body: { querySelectorAll: () => [] } }, innerWidth: width });
  return {
    style,
    get reads() { return reads; },
    async evaluate(fn, argument) {
      context.argument = argument;
      return vm.runInContext(`(${fn.toString()})(argument)`, context);
    },
  };
}

test('waits for the exact width condition after a delayed resize callback', async () => {
  const fixture = page({ transient: 2 });
  await assertViewportFit(fixture, 'owned delayed resize');
  assert.equal(fixture.reads, 3);
});

test('persistent overflow fails and the negative control restores its style', async () => {
  const fixture = page();
  await qualifyOverflowGuard(fixture);
  assert.equal(fixture.style.minWidth, '');
  assert.ok(fixture.reads > 2);
});

test('the negative control fails if overflow is incorrectly ignored', async () => {
  const fixture = page({ ignoreMinimum: true });
  await assert.rejects(() => qualifyOverflowGuard(fixture), /Missing expected rejection/);
  assert.equal(fixture.style.minWidth, '');
});
