import test from 'node:test';
import assert from 'node:assert/strict';
import { readCatalogPositions, compareCatalogBump, compareCatalogPriority } from '../../apps/public/static/catalog-preferences.v1.js';

test('catalog positions accept only a complete bounded permutation or entirely absent legacy metadata', () => {
  assert.equal(readCatalogPositions([]), null);
  assert.equal(readCatalogPositions([undefined, undefined]), null);
  assert.deepEqual(readCatalogPositions(['2', '0', '1']), [2, 0, 1]);
  const maximum = Array.from({ length: 1000 }, (_, index) => String(999 - index));
  assert.equal(readCatalogPositions(maximum)[0], 999);
  for (const values of [
    ['0', undefined], ['0', '0'], ['1'], ['0', '2'], ['0', '01'], ['0', '-1'],
    ['0', '1.0'], ['0', '1e0'], ['0', ' 1'], ['0', ''], ['0', null],
    ['0', 1], ['0', '1000'], Array(1001).fill(undefined), null,
  ]) {
    assert.throws(() => readCatalogPositions(values), /catalog position/);
  }
});

const entry = (id, bumped, position) => ({ id: BigInt(id), bumped: BigInt(bumped), position });

test('ranked bump order survives alternate sorts and GET-filtered row concatenation', () => {
  // The original first sticky has an older clock and lower ID; clocks cannot
  // reconstruct the authoritative SQL order. The array mimics visible rows
  // concatenated with the hidden template after an alternate server sort.
  const rows = [entry(30, 300, 2), entry(10, 100, 0), entry(20, 200, 1)];
  assert.deepEqual([...rows].sort(compareCatalogBump).map(row => row.id), [10n, 20n, 30n]);
  rows.sort((a, b) => Number(b.id - a.id));
  rows.sort(compareCatalogBump);
  assert.deepEqual(rows.map(row => row.position), [0, 1, 2]);
  assert.deepEqual(rows.map((row, index) => 1 + Math.floor(index / 2)), [1, 1, 2]);
});

test('both layouts use original positions; entirely absent legacy positions keep exact clock and ID tie breaks', () => {
  const rows = [entry('9007199254740992', 10, 0), entry('9007199254740993', 10, 1), entry(1, 11, 2)];
  const expected = [1n, 9007199254740993n, 9007199254740992n];
  assert.deepEqual([...rows].sort(compareCatalogBump).map(row => row.id), [9007199254740992n, 9007199254740993n, 1n]);
  assert.deepEqual(rows.map(row => ({ ...row, position: null })).sort(compareCatalogBump).map(row => row.id), expected);
});

test('pins and filter-top cannot reorder image stickies within the selected sort', () => {
  const high = { ...entry(10, 100, 0), sticky: true };
  const low = { ...entry(20, 200, 1), sticky: true };
  for (const [highTop, lowTop] of [[false, true], [true, false], [true, true], [false, false]]) {
    assert.equal(compareCatalogPriority(high, low, false, highTop, lowTop), 0);
    const bump = [low, high].sort((a, b) => compareCatalogPriority(a, b, false,
      a === high ? highTop : lowTop, b === high ? highTop : lowTop) || compareCatalogBump(a, b));
    assert.deepEqual(bump.map(row => row.id), [10n, 20n]);
    // Other selected orders, including ones that put the lower-ranked sticky
    // first, remain authoritative within the sticky bucket as in the source.
    const creation = [high, low].sort((a, b) => compareCatalogPriority(a, b, false,
      a === high ? highTop : lowTop, b === high ? highTop : lowTop) || Number(b.id - a.id));
    assert.deepEqual(creation.map(row => row.id), [20n, 10n]);
  }
});

test('ordinary and text catalog rows retain pin/filter-top priority below image stickies', () => {
  const sticky = { ...entry(10, 100, 0), sticky: true };
  const first = { ...entry(20, 200, 1), sticky: false };
  const promoted = { ...entry(30, 300, 2), sticky: false };
  assert.equal(compareCatalogPriority(sticky, promoted, false, false, true), -1);
  assert.equal(compareCatalogPriority(first, promoted, false, false, true), 1);
  assert.equal(compareCatalogPriority(sticky, promoted, true, false, true), 1);
  assert.equal(compareCatalogPriority(sticky, { ...promoted, sticky: true }, true, false, true), 1);
});
