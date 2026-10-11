import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
import { parseFragment } from 'parse5';

const fixture = JSON.parse(await readFile(new URL('../../../fixtures/source-linkification.json', import.meta.url), 'utf8'));
assert.equal(createHash('sha256').update(fixture.excerpt).digest('hex'),
  '16e8c25d177f3651e90b8ee548656129f23ff17beba92064db490a2d612cc124');
export const sourceDefaults = fixture.defaults;
const context = vm.createContext(Object.create(null), { codeGeneration: { strings: false, wasm: false } });
new vm.Script(`const Main = { board: 'g' }; const $L = { d: () => '4chan.org' };\n${fixture.excerpt}\nLinkify.init();`)
  .runInContext(context, { timeout: 1000 });
const execute = new vm.Script('{ const el = { innerHTML: input }; Linkify.exec(el); output = el.innerHTML; }');

export function sourceLinkification(input) {
  assert.ok(typeof input === 'string' && input.length <= 16000, 'Bounded synthetic reference input required');
  context.input = input;
  execute.runInContext(context, { timeout: 1000 });
  const output = context.output;
  delete context.input; delete context.output;
  return { html: output, ...linkSummary(output, '//sys.4chan.org/derefer?url=') };
}

export function linkSummary(html, prefix = '/derefer?url=') {
  const anchors = [];
  let text = '', breaks = 0;
  function visit(node) {
    if (node.nodeName === '#text') text += node.value;
    if (node.tagName === 'wbr') breaks++;
    if (node.tagName === 'a') {
      const attrs = Object.fromEntries(node.attrs.map(({ name, value }) => [name, value]));
      if (attrs.href?.startsWith(prefix)) {
        const content = (node.childNodes ?? []).map(function content(child) {
          return child.nodeName === '#text' ? child.value : (child.childNodes ?? []).map(content).join('');
        }).join('');
        anchors.push({ label: content, parameter: attrs.href.slice(prefix.length), target: attrs.target,
          rel: attrs.rel.split(/\s+/).filter(value => value !== 'noopener').sort() });
      }
    }
    for (const child of node.childNodes ?? []) visit(child);
  }
  visit(parseFragment(html));
  return { anchors, text, breaks };
}

export const sourceLinkInputs = [
  'https://example.test/path',
  'HTTPS://UPPER.TEST/path',
  'https://lower.test HTTPS://UPPER.TEST/path',
  'https://example.test/a?a=1&amp;b=2',
  'https://example.test/a&lt;b',
  'https://example.test/a&gt;b',
  'https://example.test/a&quot;b',
  'https://example.test/a&#039;b',
  'https://example.test/a\u00a0b',
  'https://example.test/a&nbsp;b',
  'https://example.test/a\u00a0',
  'https://example.test/ab\u00a0cd<wbr>ef',
  'https://example.test/ab<wbr>cd x\u200by',
  'https://exam<wbr>ple.test/path',
  'https://seed.test https://exam<wbr>ple.test/path',
  '<s>https://example.test/spoiler</s>',
  '<span class="quote">&gt;https://example.test/green</span>',
  '<a href="https://existing.test/path">https://existing.test/path</a>',
  '<a href="https://existing.test/path">https://existing.test/path</a> https://next.test/path',
  'https://example.test:8443/path',
  'http://127.0.0.1/path',
  'https://user:pass@example.test/path',
  ...['', 'B', 'x', '"', '(', '&gt;', '<br>', '\n'].flatMap(prefix =>
    ['', '.', '?!', ')', ')).', '/a(b))', '/a(b)).', '/a)b)', '/a%20b', '/a,b'].map(suffix =>
      `${prefix}https://example.test${suffix}`)),
  ...Array.from({ length: 64 }, (_, index) =>
    `(https://example.test/${'('.repeat(index)}a${')'.repeat(index + 2)}${index % 2 ? '?!' : ''})`),
];
