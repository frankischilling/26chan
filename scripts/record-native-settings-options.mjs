import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

const root = new URL('../', import.meta.url);
const sourceHash = '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31';
const hashes = {
  defaults: '552b2a9ff0c531ca672fdf302b48ea611057cadf821925fab38fa7717f115acb',
  mobile: '9835771f2de8d3ea9a66d8714fc2a2d0a020857947254c56fb37ce9e72821f7e',
  options: 'c66ba7040bdf4845bb3d6f0a5927869ba2e1cfbdfc2d5e3e647f03a22e3646dc',
};
const hash = value => createHash('sha256').update(value).digest('hex');

// Read a finite data-literal subset. Never execute the original script.
export function parseLiteral(input) {
  assert.ok(typeof input === 'string' && input.length <= 32768);
  let at = 0, items = 0;
  function space() {
    for (;;) {
      while (/\s/.test(input[at] || '') && at < input.length) at++;
      if (!input.startsWith('//', at)) return;
      const end = input.indexOf('\n', at);
      at = end < 0 ? input.length : end + 1;
    }
  }
  function string() {
    const quote = input[at++]; let value = '';
    while (at < input.length) {
      const ch = input[at++];
      if (ch === quote) return value;
      if (ch === '\\') {
        const escaped = input[at++];
        assert.ok(["'", '"', '\\'].includes(escaped), 'Unexpected string escape');
        value += escaped;
      } else {
        assert.ok(ch !== '\n' && ch !== '\r', 'Unexpected string newline');
        value += ch;
      }
    }
    assert.fail('Unclosed string');
  }
  function identifier() {
    const match = /^[A-Za-z_$][\w$]*/.exec(input.slice(at));
    assert.ok(match, 'Expected literal identifier'); at += match[0].length; return match[0];
  }
  function value(depth = 0) {
    space(); assert.ok(depth <= 4 && ++items <= 1024, 'Literal limit');
    if (input[at] === "'" || input[at] === '"') return string();
    if (input[at] === '{' || input[at] === '[') {
      const object = input[at++] === '{', close = object ? '}' : ']';
      const result = object ? Object.create(null) : [];
      space();
      while (input[at] !== close) {
        if (object) {
          const key = input[at] === "'" || input[at] === '"' ? string() : identifier();
          assert.ok(!Object.hasOwn(result, key), 'Duplicate key');
          space(); assert.equal(input[at++], ':'); result[key] = value(depth + 1);
        } else result.push(value(depth + 1));
        space(); if (input[at] === close) break;
        assert.equal(input[at++], ','); space();
      }
      at++; return result;
    }
    const token = identifier(); assert.ok(token === 'true' || token === 'false', 'Only boolean values are admitted');
    return token === 'true';
  }
  const result = value(); space(); assert.equal(at, input.length, 'Trailing source data'); return result;
}

export function extractSource(bytes) {
  assert.equal(hash(bytes), sourceHash, 'Unexpected pinned source');
  const source = bytes.toString('utf8').replaceAll('\r\n', '\n');
  const parts = {};
  for (const [key, start, end] of [
    ['defaults', 'var Config = {', '\n\nvar ConfigMobile'],
    ['mobile', 'var ConfigMobile = {', '\n\nConfig.load'],
    ['options', 'SettingsMenu.options = {', '\n\nSettingsMenu.save'],
  ]) {
    const from = source.indexOf(start), to = source.indexOf(end, from);
    assert.ok(from >= 0 && to > from); parts[key] = source.slice(from, to);
  }
  return parts;
}
export function buildReference(snippets) {
  assert.deepEqual(Object.keys(snippets).sort(), Object.keys(hashes).sort());
  const data = {};
  for (const [key, expected] of Object.entries(hashes)) {
    const text = snippets[key]; assert.equal(hash(text), expected, `Changed snippet: ${key}`);
    assert.ok(text.endsWith('};'));
    data[key] = parseLiteral(text.slice(text.indexOf('{'), -1));
  }
  const groups = Object.entries(data.options).map(([name, options]) => ({
    name: name.replaceAll('&amp;', '&'),
    options: Object.entries(options).map(([key, tuple]) => {
      assert.ok(Array.isArray(tuple) && tuple.length >= 2 && tuple.length <= 5);
      assert.equal(typeof tuple[0], 'string'); assert.equal(typeof tuple[1], 'string');
      assert.ok(tuple.slice(2).every(item => typeof item === 'boolean'));
      return { key, label: tuple[0], description: tuple[1], mobile: tuple[2] === true,
        suboption: tuple[3] === true, mobileOnly: tuple[4] === true };
    }),
  }));
  return { source: 'js/extension.js', revision: '545b7812d1849f7958d914950c91fdbbe38f6b22',
    sha256: sourceHash, defaults: data.defaults, mobile: data.mobile, groups };
}
async function main() {
  const args = process.argv.slice(2), check = args[0] === '--check';
  if (check) args.shift();
  let source;
  if (args.length) { assert.equal(args.shift(), '--source'); source = args.shift(); assert.ok(source); }
  assert.equal(args.length, 0);
  const retained = JSON.parse(await readFile(new URL('tests/fixtures/native-settings-options-snippets.json', root), 'utf8'));
  if (source) assert.deepEqual(extractSource(await readFile(source)), retained);
  const result = JSON.stringify(buildReference(retained), null, 2) + '\n';
  const target = new URL('tests/fixtures/native-settings-options-source.json', root);
  if (check) assert.equal(await readFile(target, 'utf8'), result, 'Stale source settings fixture');
  else await writeFile(target, result);
  console.log(`Pinned Settings defaults and complete option matrix ${check ? 'verified' : 'written'}.`);
}
if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) await main();
