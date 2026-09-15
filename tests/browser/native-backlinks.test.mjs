import test from 'node:test';
import assert from 'node:assert/strict';
import { BACKLINK_LIMITS, mountNativeBacklinks } from '../../apps/public/client/native-backlinks.js';
import { markNativeTrackedQuotes } from '../../apps/public/client/native-tracked-quotes.js';
import { FILTER_LIMITS } from '../../apps/public/client/native-filter-limits.js';

test('backlink admission rejects invalid authority before touching a document', () => {
  const root = { matches() { throw new Error('document was touched'); } };
  const options = { root, origin: 'https://board.example', board: 'demo', thread: '1',
    quoteTarget() {}, settings: () => ({}) };
  for (const patch of [{ board: '../demo' }, { board: 'demo\n' }, { board: '' }, { board: 'TOOLONGBOARD' },
    { thread: 1 }, { thread: '01' }, { thread: '0' }, { thread: '1\n' }, { thread: '9223372036854775808' },
    { origin: 'https://user:password@board.example' }, { origin: 'https://board.example/path' },
    { origin: 'data:text/html,hello' }, { origin: 'https://board.example/' },
    { settings: null }, { quoteTarget: null }, { root: null }]) {
    assert.equal(mountNativeBacklinks({ ...options, ...patch }), null, JSON.stringify(patch));
  }
  assert.equal(mountNativeBacklinks(), null);
});

test('page-only admission accepts adjacent maximum i64 identities but excludes catalogs', () => {
  for (const thread of ['9007199254740992', '9007199254740993', '9223372036854775807', null]) {
    let matched = 0;
    assert.equal(mountNativeBacklinks({ root: { matches() { matched++; return false; } },
      origin: 'https://board.example', board: 'demo', thread, quoteTarget() {}, settings() {} }), null);
    assert.equal(matched, 1);
  }
  assert.equal(mountNativeBacklinks({ root: { matches: () => true, closest: () => ({}) },
    origin: 'https://board.example', board: 'demo', quoteTarget() {}, settings() {} }), null);
});

test('tracked quote callbacks compose labels without recognizing arbitrary original suffix text', () => {
  const classes = new Set(), writes = [];
  const link = { dataset: {}, textContent: '>>9223372036854775807 (OP)',
    classList: { add: name => classes.add(name), remove: name => classes.delete(name) } };
  const section = { querySelectorAll: () => [link] };
  let base = '>>9223372036854775807';
  const callbacks = { readLabel: () => base, writeLabel: (_node, text) => {
    writes.push(text); base = text; link.textContent = `${text} (OP)`;
  } };
  markNativeTrackedQuotes(section, new Set(['9223372036854775807']), true, callbacks);
  markNativeTrackedQuotes(section, new Set(['9223372036854775807']), true, callbacks);
  assert.equal(link.textContent, '>>9223372036854775807 (You) (OP)');
  assert.equal(writes.length, 1); assert.ok(classes.has('ql-tracked'));
  markNativeTrackedQuotes(section, new Set(), false, callbacks);
  assert.equal(link.textContent, '>>9223372036854775807 (OP)');
  assert.equal(classes.has('ql-tracked'), false); assert.equal(link.dataset.nativeTracked, undefined);
  base = '>>9223372036854775807 (OP)';
  markNativeTrackedQuotes(section, new Set(['9223372036854775807']), true, callbacks);
  assert.equal(writes.length, 2, 'original OP text is not silently parsed as an owned suffix');
});

test('backlink budgets preserve existing filter ceilings and bound all added rows', () => {
  assert.ok(Object.isFrozen(BACKLINK_LIMITS));
  assert.equal(BACKLINK_LIMITS.html, FILTER_LIMITS.html);
  assert.equal(BACKLINK_LIMITS.text, FILTER_LIMITS.field);
  assert.ok(BACKLINK_LIMITS.previewRows * 6 + 1 <= BACKLINK_LIMITS.previewNodes);
  for (const value of Object.values(BACKLINK_LIMITS)) assert.ok(Number.isSafeInteger(value) && value > 0);
});
