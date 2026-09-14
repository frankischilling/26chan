import assert from 'node:assert/strict';
import test from 'node:test';
import { nativeShortcut, siblingPageUrl } from './native-keybinds.js';

const event = (keyCode, extra = {}) => ({ keyCode, target: { nodeName: 'BODY' }, ...extra });
const enabled = { keyBinds: true };

test('native shortcuts are optional, case-sensitive settings and globally disabled together', () => {
  for (const settings of [null, {}, { keyBinds: false }, { keybinds: true }, { keyBinds: 'true' }, { ...enabled, disableAll: true }]) {
    assert.equal(nativeShortcut(event(87), settings), null);
  }
  assert.equal(nativeShortcut(event(87), enabled), 'watch');
});

test('the pinned numeric key map includes feature-gated commands without inventing aliases', () => {
  for (const [code, name] of [[65, 'auto'], [70, 'filter'], [81, 'quickReply'], [82, 'update'], [87, 'watch'],
    [66, 'previous'], [67, 'catalog'], [78, 'next'], [73, 'index']]) {
    assert.equal(nativeShortcut(event(code), enabled), name);
  }
  for (const key of [0, 27, 83, 229, '87', null, undefined]) assert.equal(nativeShortcut(event(key), enabled), null);
});

test('inputs, textareas and every modifier retain their ordinary keyboard behavior', () => {
  for (const nodeName of ['INPUT', 'TEXTAREA']) {
    assert.equal(nativeShortcut(event(87, { target: { nodeName } }), enabled), null);
  }
  for (const modifier of ['altKey', 'shiftKey', 'ctrlKey', 'metaKey']) {
    assert.equal(nativeShortcut(event(87, { [modifier]: true }), enabled), null);
  }
  // The reference excludes INPUT/TEXTAREA, not every focusable element.
  assert.equal(nativeShortcut(event(87, { target: { nodeName: 'BUTTON' } }), enabled), 'watch');
  assert.equal(nativeShortcut(event(87, { target: { nodeName: 'SELECT' } }), enabled), 'watch');
});

test('sibling navigation accepts only bounded page links on the current board and origin', () => {
  const origin = 'http://127.0.0.1:3000';
  for (const path of ['/demo/', '/demo/0', '/demo/1', '/demo/2/', '/demo/2147483647']) {
    assert.equal(siblingPageUrl(origin, 'demo', path), origin + path);
  }
  for (const path of ['javascript:void(0)', '//example.invalid/demo/1', '/test/1', '/demo/catalog',
    '/demo/thread/1', '/demo/01', '/demo/2147483648', '/demo/1?token=x', '/demo/1#p1',
    'http://user:pass@127.0.0.1:3000/demo/1', '/demo/%31', null]) {
    assert.equal(siblingPageUrl(origin, 'demo', path), null);
  }
  assert.equal(siblingPageUrl(origin, '../demo', '/demo/1'), null);
  assert.equal(siblingPageUrl('file:///', 'demo', '/demo/1'), null);
});
