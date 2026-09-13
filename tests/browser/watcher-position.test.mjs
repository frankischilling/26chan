import test from 'node:test';
import assert from 'node:assert/strict';
import { readWatcherPosition, writeWatcherPosition, dragWatcherPosition } from '../../apps/public/static/watcher-position.v1.js';

test('native pixel, percentage and opposite-edge positions round-trip without taking over fixed mode', () => {
  assert.deepEqual(readWatcherPosition('left: 12.5%; top: 75px; position: fixed;'), { left: '12.5%', top: '75px' });
  assert.deepEqual(readWatcherPosition('right: 0; bottom: 0px;'), { right: '0px', bottom: '0px' });
  const position = { left: '25%', bottom: '0px' };
  assert.deepEqual(readWatcherPosition(writeWatcherPosition(position, true)), position);
});

test('stored position text cannot supply arbitrary CSS, ambiguous axes or unbounded values', () => {
  for (const raw of [null, {}, '', 'left: 1px;', 'left: -1px; top: 0;', 'left: 1e9px; top: 0;',
    'left: 2; top: 0;', 'left: 1000001px; top: 0;', 'left: 10001%; top: 0;',
    'left: 0; right: 0; top: 0;', 'left: 0; left: 1px; top: 0;',
    'left: calc(1px); top: 0;', 'left: var(--x); top: 0;', 'left: 0; top: 0; color: red;',
    'left: 0; top: 0; background: url(https://example.invalid/);',
    'left: 0; top: 0; position: sticky;', 'left: 0; top: 0; position: fixed; position: absolute;',
    'left: 0; top: 0;' + ' '.repeat(257)]) assert.equal(readWatcherPosition(raw), null, String(raw));
});

const geometry = { width: 1000, height: 800, panelWidth: 200, panelHeight: 100,
  dx: 10, dy: 10, scrollX: 0, scrollY: 0, offsetTop: 0 };

test('dragging uses native percentages inside the viewport and anchors at its edges', () => {
  assert.deepEqual(dragWatcherPosition(260, 210, geometry), { left: '25%', top: '25%' });
  assert.deepEqual(dragWatcherPosition(-50, -50, geometry), { left: '0px', top: '0px' });
  assert.deepEqual(dragWatcherPosition(9999, 9999, geometry), { right: '0px', bottom: '0px' });
});

test('absolute drag coordinates include captured scroll offsets and fixed coordinates do not', () => {
  assert.deepEqual(dragWatcherPosition(110, 110, { ...geometry, scrollX: 50, scrollY: 100 }), { left: '15%', top: '25%' });
  assert.deepEqual(dragWatcherPosition(110, 110, geometry), { left: '10%', top: '12.5%' });
});

test('tall panels follow the native top-position branch rather than forcing a bottom anchor', () => {
  assert.deepEqual(dragWatcherPosition(110, 910, { ...geometry, panelHeight: 1000 }), { left: '10%', top: '112.5%' });
  assert.deepEqual(dragWatcherPosition(110, 20, { ...geometry, offsetTop: 30 }), { left: '10%', top: '30px' });
});

test('non-finite geometry is rejected before creating style values', () => {
  assert.equal(dragWatcherPosition(Infinity, 20, geometry), null);
  assert.equal(dragWatcherPosition(20, 20, { ...geometry, width: 0 }), null);
  assert.equal(dragWatcherPosition(20, 20, { ...geometry, scrollY: NaN }), null);
});
