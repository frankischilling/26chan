import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { relativePostAge } from '../../apps/public/static/native-post-tooltips.v1.js';

const reference = JSON.parse(await readFile(new URL('../../docs/public-post-tooltip-reference.json', import.meta.url), 'utf8'));

test('relative dates match the released helper at its unit boundaries', () => {
  const timestamp = 1801324800;
  for (const { delta, text } of reference.ages) {
    assert.equal(relativePostAge(timestamp, timestamp * 1000 + delta * 1000), text, `delta ${delta}`);
  }
});

test('relative dates reject invalid or unbounded timestamp and clock values', () => {
  for (const value of [-1, '1801324800', 1.5, NaN, Infinity, 8_640_000_000_001]) {
    assert.equal(relativePostAge(value, 1801324800000), null);
  }
  for (const now of [-1, '1801324800000', NaN, Infinity, 8_640_000_000_000_001]) {
    assert.equal(relativePostAge(1801324800, now), null);
  }
  assert.equal(relativePostAge(0, 0), 'moments ago');
  assert.equal(relativePostAge(8_640_000_000_000, 8_640_000_000_000_000), 'moments ago');
});
