import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import vm from 'node:vm';

const root = new URL('../', import.meta.url);
const hashes = {
  board: '79d42880882ce963bcd9a3ed85414a583453be2e5205f7640c834a6ab8201114',
  catalog: 'e2c63b483b27f79100c1a9c963acece6f9c30a0eea772b61bf871b4a2bf33301',
  mobile: '2f3608b38777e38a2f5ebe1da645460672dab1daadd119b1d4e3e1d1d538e6cf',
};
const sources = {
  board: ['js/extension.js', '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31'],
  catalog: ['js/catalog.js', '12f59335953bd013ce892a24186218a31f2e8ad7fe3dbaa54af82f40ca182e5e'],
  mobile: ['js/core.js', 'a9ab67bea1f51fcdaaac1a879098a4d5888622008bf7f26f1fe18861ea52aab5'],
};
const hash = value => createHash('sha256').update(value).digest('hex');
export function buildReference(snippets) {
  assert.deepEqual(Object.keys(snippets).sort(), Object.keys(hashes).sort());
  for (const [key, digest] of Object.entries(hashes)) {
    assert.equal(hash(snippets[key].snippet), digest);
    assert.equal(snippets[key].snippetSha256, digest);
    assert.deepEqual([snippets[key].file, snippets[key].fileSha256], sources[key]);
  }
  const cases = [];
  for (const mode of ['board', 'catalog']) for (const current of ['1700000000', '1700000001', '9']) {
    for (const previous of [null, '', '1699999999', '1700000000', '1700000001', '10']) {
      const nodes = {}, stored = new Map(previous === null ? [] : [['4chan-global-msg', previous]]);
      const message = { textContent: 'Owned announcement', style: { display: '' },
        nextElementSibling: { style: { clear: '' } }, getAttribute: name => name === 'data-utc' ? current : null,
        parentNode: { insertBefore: button => { nodes[button.id] = button; } } };
      nodes.globalMessage = message;
      const context = { Main: { icons: { plus: 'plus', minus: 'minus' } },
        UA: { hasWebStorage: true }, FC: { hasMobileLayout: false },
        document: { createElement: () => ({ style: {}, setAttribute() {} }) },
        $: { id: id => nodes[id], on() {} },
        localStorage: { getItem: key => stored.get(key) ?? null,
          setItem: (key, value) => stored.set(key, value), removeItem: key => stored.delete(key) } };
      vm.createContext(context, { codeGeneration: { strings: false, wasm: false } });
      vm.runInContext(snippets[mode].snippet, context, { timeout: 50 });
      const init = mode === 'board' ? 'Main.initGlobalMessage()' : 'initGlobalMessage()';
      const toggle = mode === 'board' ? 'Main.toggleGlobalMessage()' : 'toggleGlobalCatalogMessage()';
      vm.runInContext(init, context, { timeout: 50 });
      const states = [];
      const capture = () => states.push({ hidden: message.style.display === 'none', stored: stored.get('4chan-global-msg') ?? null });
      capture(); vm.runInContext(toggle, context, { timeout: 50 }); capture();
      vm.runInContext(toggle, context, { timeout: 50 }); capture();
      cases.push({ mode, current, previous, states });
    }
  }
  const message = { className: 'globalMessage hideMobile' };
  const button = { className: 'button redButton mobile hidden', innerHTML: 'View Important Announcement' };
  const context = { document: { getElementById: id => id === 'globalToggle' ? button : message } };
  vm.createContext(context, { codeGeneration: { strings: false, wasm: false } });
  vm.runInContext(snippets.mobile.snippet, context, { timeout: 50 });
  const mobile = [];
  for (let i = 0; i < 2; i++) {
    vm.runInContext('toggleGlobalMessage()', context, { timeout: 50 });
    mobile.push({ hidden: message.className.includes('hideMobile'), label: button.innerHTML });
  }
  return { revision: '545b7812d1849f7958d914950c91fdbbe38f6b22',
    scope: 'Only pinned announcement UI functions with synthetic DOM/storage; no application, network, database or original content.',
    cases, mobile };
}
async function main() {
  assert.ok(process.argv.length === 2 || (process.argv.length === 3 && process.argv[2] === '--check'));
  const snippets = JSON.parse(await readFile(new URL('tests/fixtures/global-announcement-snippets.json', root), 'utf8'));
  const output = JSON.stringify(buildReference(snippets), null, 2) + '\n';
  const target = new URL('tests/fixtures/global-announcement-source.json', root);
  if (process.argv[2] === '--check') assert.equal(await readFile(target, 'utf8'), output);
  else await writeFile(target, output);
  console.log('Pinned announcement desktop/catalog/mobile reference verified.');
}
if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) await main();
