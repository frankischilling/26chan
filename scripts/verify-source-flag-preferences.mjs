import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import vm from 'node:vm';

assert.equal(process.argv.length, 4, 'Provide the supplied core.js and extension.js paths.');
const [coreRaw, extensionRaw] = await Promise.all(process.argv.slice(2).map(path => readFile(path, 'utf8')));
for (const text of [coreRaw, extensionRaw]) assert.ok(Buffer.byteLength(text) <= 4 * 1024 * 1024);
const [core, extension] = [coreRaw, extensionRaw].map(text => text.replace(/\r\n?/g, '\n'));
const callback = /function onBoardFlagChanged\(\) \{[\s\S]{1,500}?\n\}/.exec(core)?.[0];
const restoration = /\/\/ Selectable flags\n([\s\S]{1,800}?)\/\/ Mobile nav menu/.exec(core)?.[1];
const qr = /else if \(\(el.name == 'flag'\)\) \{([\s\S]{1,800}?)\n        \}/.exec(extension)?.[1];
assert.ok(callback && restoration && qr, 'Source preference helpers differ.');
const reference = JSON.parse(await readFile(new URL('../apps/public/tests/fixtures/board-flags.json', import.meta.url), 'utf8'));
const stored = new Map(), listeners = new Map();
const context = vm.createContext({
  document: { forms: {} }, location: {}, Main: {}, board: '',
  localStorage: {
    getItem: key => stored.get(key) ?? null,
    setItem: (key, value) => stored.set(key, value),
    removeItem: key => stored.delete(key),
  },
  $: { qs: (selector, field) => field.querySelector(selector),
    on: (field, event, listener) => field.addEventListener(event, listener) },
});
context.window = context;
vm.runInContext(callback, context, { timeout: 100 });
const restore = new vm.Script(`(function() { var el, val, el2; ${restoration} })()`);
const restoreQR = new vm.Script(`(function() { var el2, cookie; ${qr} })()`);
let count = 0;
for (const [kind, table] of Object.entries(reference.tables)) {
  context.board = context.Main.board = kind;
  context.location.pathname = `/${kind}/thread/1000`;
  for (const code of table.selector_order) {
    const field = { name: 'flag', value: '0',
      addEventListener: (event, listener) => listeners.set(event, listener),
      querySelector(selector) {
        if (selector === 'option[selected]') return { removeAttribute() {} };
        const wanted = /^option\[value="([A-Z0-9]{1,3})"\]$/.exec(selector)?.[1];
        if (!wanted || !['0', ...table.selector_order].includes(wanted)) return null;
        return { setAttribute(name, value) {
          assert.equal(name, 'selected'); assert.equal(value, 'selected'); field.value = wanted;
        } };
      },
    };
    context.document.forms.post = { flag: field, querySelector: selector => field.querySelector(selector) };
    context.el = field;
    const key = `4chan_flag_${kind}`;
    stored.clear(); stored.set(key, code);
    restore.runInContext(context, { timeout: 100 });
    assert.equal(field.value, code, `${kind}/${code}: core restoration`);
    field.value = '0';
    restoreQR.runInContext(context, { timeout: 100 });
    assert.equal(field.value, code, `${kind}/${code}: Quick Reply restoration`);
    assert.equal(listeners.get('change'), context.onBoardFlagChanged);
    stored.clear();
    context.onBoardFlagChanged.call(field);
    assert.deepEqual([...stored], [[key, code]], `${kind}/${code}: board-scoped write`);
    field.value = '0'; context.onBoardFlagChanged.call(field);
    assert.equal(stored.size, 0, `${kind}/${code}: clearing the preference`);
    count++;
  }
}
assert.equal(count, 165);
console.log(`Verified both supplied restorers, board-scoped writes and removal for ${count} flag choices.`);
for (const [name, value] of [['core.js', coreRaw], ['extension.js', extensionRaw]]) {
  console.log(`${name} SHA-256: ${createHash('sha256').update(value).digest('hex')}`);
}
