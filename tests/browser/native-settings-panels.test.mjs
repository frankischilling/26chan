import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { buildReference, extractSource } from '../../scripts/record-native-settings-panels.mjs';
import { mountNativeKeybinds } from '../../apps/public/client/native-keybinds.js';

const root = new URL('../../', import.meta.url);
const reference = JSON.parse(await readFile(new URL('tests/fixtures/native-settings-panel-source.json', root), 'utf8'));
const manifest = JSON.parse(await readFile(new URL('docs/public-watcher-assets.json', root), 'utf8'));

test('all selected panel images retain their recorded bytes, dimensions and fixed local URLs', async () => {
  for (const [theme, family] of Object.entries(reference.families)) {
    const css = await readFile(new URL(theme === 'yotsuba' ? 'apps/public/static/board.css'
      : `apps/public/static/themes/${theme}.css`, root), 'utf8');
    for (const suffix of ['', '@2x']) {
      for (const [property, name] of [['watcher-cross', 'cross'], ['settings-plus', 'post_expand_plus'], ['settings-minus', 'post_expand_minus']]) {
        const path = `${family}/${name}${suffix}.png`;
        assert.ok(css.includes(`--${property}: url('/static/watcher/${path}')`), `${theme}: ${path}`);
        const expected = manifest.assets.find(asset => asset.name === path);
        assert.ok(expected, path);
        const bytes = await readFile(new URL(`apps/public/static/watcher/${path}`, root));
        assert.equal(bytes.length, expected.bytes);
        assert.equal(createHash('sha256').update(bytes).digest('hex'), expected.sha256);
        assert.deepEqual([bytes.readUInt32BE(16), bytes.readUInt32BE(20)], suffix ? [36, 36] : [18, 18]);
      }
    }
  }
});

const snippets = JSON.parse(await readFile(new URL('tests/fixtures/native-settings-panel-snippets.json', root), 'utf8'));

test('retained pinned snippets reproduce the complete fixture and reject drift', () => {
  assert.deepEqual(buildReference(snippets), reference);
  for (const key of Object.keys(snippets)) assert.throws(() => buildReference({ ...snippets, [key]: snippets[key] + ' ' }), /Changed source snippet/);
  assert.throws(() => extractSource(Buffer.from(snippets.desktop)), /Unexpected pinned source bytes/);
});

test('source fixture retains the pinned desktop and mobile rules, including Settings-only positioning', () => {
  assert.equal(reference.sha256, '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31');
  assert.match(reference.css, /\.UIPanel > div \{[^}]*width: 400px;[^}]*vertical-align: middle;/s);
  assert.match(reference.css, /#settingsMenu > div \{\s+top: 25px;;\s+vertical-align: top;\s+max-height: 85%;/);
  assert.match(reference.css, /@media only screen and \(max-width: 480px\).*width: 320px;/s);
});

// Minimal event/ownership harness. Browser tests separately check layout and native focus.
function harness() {
  const nodes = [];
  class Node {
    constructor(tag) { this.tagName = tag.toUpperCase(); this.children = []; this.listeners = new Map(); this.classes = new Set(); this.classList = { add: name => this.classes.add(name) }; }
    setAttribute(key, value) { this[key] = value; }
    append(...items) { for (const item of items) { this.children.push(item); if (typeof item === 'object') item.parent = this; } }
    addEventListener(name, fn) { this.listeners.set(name, fn); }
    removeEventListener(name, fn) { if (this.listeners.get(name) === fn) this.listeners.delete(name); }
    showModal() { this.open = true; }
    close() { this.open = false; }
    remove() { this.removed = true; }
    focus() { this.focused = (this.focused || 0) + 1; }
    dispatch(type, target = this) { let prevented = false; this.listeners.get(type)?.({ target, preventDefault() { prevented = true; } }); return prevented; }
  }
  const document = new Node('document');
  document.body = new Node('body');
  document.createElement = tag => { const el = new Node(tag); nodes.push(el); return el; };
  const window = new Node('window');
  return { document, window, nodes, opener: new Node('a') };
}

test('keyboard help closes only for backdrop, Close or cancelled Escape and restores its opener once', () => {
  const previous = { document: globalThis.document, window: globalThis.window };
  const { document, window, nodes, opener } = harness();
  Object.assign(globalThis, { document, window });
  try {
    const controller = mountNativeKeybinds({ board: 'demo', settings: () => ({}) });
    for (const action of ['backdrop', 'close', 'escape']) {
      controller.openHelp(opener);
      const dialog = nodes.filter(node => node.id === 'keybindsHelp').at(-1);
      const panel = dialog.children[0];
      const text = node => typeof node === 'string' ? node : (node.textContent || '') + node.children.map(text).join('');
      const rows = panel.children.filter(node => node.tagName === 'UL').flatMap(list => list.children)
        .filter(row => row.children[0]?.tagName === 'KBD').map(text);
      assert.deepEqual(rows, reference.shortcuts, 'source UI separators and labels');
      const close = nodes.filter(node => node.id === 'keybinds-close').at(-1);
      assert.equal(dialog.open, true);
      assert.ok(close.classes.has('nativePanelClose'));
      assert.equal(close['aria-label'], 'Close keyboard shortcuts');
      dialog.dispatch('click', panel);
      assert.equal(dialog.open, true);
      const count = nodes.length;
      controller.openHelp(opener);
      assert.equal(nodes.length, count, 'reopening does not create another dialog');
      const focusBefore = opener.focused || 0;
      if (action === 'backdrop') dialog.dispatch('click');
      else if (action === 'close') close.dispatch('click');
      else assert.equal(dialog.dispatch('cancel'), true);
      assert.equal(dialog.open, false);
      assert.equal(dialog.removed, true);
      assert.equal(opener.focused, focusBefore + 1);
      dialog.dispatch('click');
      assert.equal(opener.focused, focusBefore + 1);
    }
  } finally {
    for (const [key, value] of Object.entries(previous)) {
      if (value === undefined) delete globalThis[key]; else globalThis[key] = value;
    }
  }
});
