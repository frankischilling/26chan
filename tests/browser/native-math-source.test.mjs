import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import vm from 'node:vm';

const oracle = JSON.parse(await readFile(new URL('../fixtures/native-math-source.json', import.meta.url)));
const snippets = Object.fromEntries(Object.entries(oracle.snippets).map(([name, value]) => [name, value.text]));

test('legacy oracle independently pins original bytes and deliberately excludes rendering claims', () => {
  assert.equal(oracle.source_revision, '545b7812d1849f7958d914950c91fdbbe38f6b22');
  for (const row of Object.values(oracle.snippets)) {
    assert.equal(Buffer.byteLength(row.text), row.byte_end - row.byte_start);
    assert.equal(createHash('sha256').update(row.text).digest('hex'), row.sha256);
    assert.ok(oracle.files[row.file]);
  }
  assert.match(oracle.scope, /Not a MathJax renderer oracle/);
});

test('actual source detector requires lowercase explicit tags, supports initial legacy math nodes', () => {
  let posts = [];
  const sandbox = { document: { getElementsByClassName: () => posts } };
  vm.createContext(sandbox);
  vm.runInContext(snippets.core, sandbox, { timeout: 100 });
  for (const [html, expected] of [['plain x^2', false], ['$x$', false], ['\\(x\\)', false],
    ['[math]x[/math]', true], ['[eqn]x[/eqn]', true], ['[math]', true],
    ['[MATH]x[/MATH]', false], ['<span class="math">x</span>', true]]) {
    posts = [{ innerHTML: html }];
    assert.equal(sandbox.pageHasMath(), expected, html);
  }
  posts = [];
  assert.equal(sandbox.pageHasMath(), false);
});

test('source detection admits nested and malformed lowercase openers without assigning display semantics', () => {
  let posts = [];
  const sandbox = { document: { getElementsByClassName: () => posts } };
  vm.createContext(sandbox);
  vm.runInContext(snippets.core, sandbox, { timeout: 100 });
  for (const input of ['[math]a[math]b[/math]c[/math]', '[eqn]a[eqn]b[/eqn]c[/eqn]',
    '[math]a[eqn]b[/eqn]c[/math]', '[eqn]a[math]b[/math]c[/eqn]',
    '[math]a[eqn]b[/math]c[/eqn]', '[math]a[eqn]b[/eqn]',
    '[math]'.repeat(4096), '[eqn]a[/math]']) {
    posts = [{ innerHTML: input }];
    assert.equal(sandbox.pageHasMath(), true, input.slice(0, 80));
  }
  for (const input of ['[/math][/eqn]', '[MATH][EQN]a[/EQN][/MATH]']) {
    posts = [{ innerHTML: input }];
    assert.equal(sandbox.pageHasMath(), false, input);
  }
  // The server invocation is disabled; the dormant PHP depth-two helper does
  // not specify how the remote renderer would have displayed nested tags.
  assert.match(snippets.server_disabled_call, /^  \/\*[\s\S]*jsmath_parse[\s\S]*\*\//);
});

test('actual source loader configures explicit delimiters and disables unsafe macro capabilities', () => {
  const appended = [];
  const sandbox = { document: {
    getElementsByTagName: () => [{ appendChild: node => appended.push(node) }],
    createElement: tag => ({ tag }),
  } };
  vm.createContext(sandbox);
  vm.runInContext(snippets.core, sandbox, { timeout: 100 });
  sandbox.loadMathJax();
  let config;
  vm.runInNewContext(appended[0].text, { MathJax: { Hub: { Config: value => { config = value; } } } }, { timeout: 100 });
  assert.deepEqual(JSON.parse(JSON.stringify(config.tex2jax.inlineMath)), [['[math]', '[/math]']]);
  assert.deepEqual(JSON.parse(JSON.stringify(config.tex2jax.displayMath)), [['[eqn]', '[/eqn]']]);
  assert.equal(config.tex2jax.processRefs, false);
  assert.equal(config.tex2jax.processEnvironments, false);
  assert.equal(config.displayAlign, 'left');
  for (const key of ['URLs', 'classes', 'cssIDs', 'styles', 'fontsize', 'require']) assert.equal(config.Safe.allow[key], 'none');
  for (const key of ['color', 'newcommand', 'renewcommand', 'newenvironment', 'renewenvironment', 'def', 'let']) assert.equal(config.TeX.Macros[key], '{}');
  assert.match(appended[1].src, /mathjax\/2\.6-latest/); // Evidence only; never fetch the legacy CDN.
});

test('source initial math does not depend on extension preferences; server keeps tags literal', () => {
  assert.match(snippets.startup, /window\.math_tags && pageHasMath\(\)/);
  assert.doesNotMatch(snippets.startup, /disableAll|Config/);
  assert.match(snippets.server_disabled_call, /^  \/\*[\s\S]*jsmath_parse[\s\S]*\*\//);
  assert.match(snippets.page_policy, /var math_tags = true/);
  assert.match(snippets.api_policy, /\['math_tags'\] = 1/);
  assert.deepEqual(oracle.policies['config/global_config.ini'], ['JSMATH = no']);
  assert.deepEqual(oracle.policies['config/boards/sci.config.ini'], ['JSMATH = yes']);
  assert.deepEqual(oracle.policies['config/boards/test.config.ini'], [';JSMATH = yes']);
});

test('source dynamic typesetting is lazy and QR has a separate empty input with 50ms debounce', () => {
  assert.match(snippets.dynamic, /if \(window\.math_tags\)/);
  assert.match(snippets.dynamic, /Parser\.postHasMath\(posts\[i\]\)/);
  assert.match(snippets.dynamic, /window\.loadMathJax\(\)/);
  assert.match(snippets.preview, /<textarea id="input-tex-preview"><\/textarea>/);
  assert.match(snippets.preview, /clearTimeout\(QR\.timeoutTeX\)/);
  assert.match(snippets.preview, /setTimeout\(QR\.processTeX, 50\)/);
  assert.match(snippets.preview, /dest\.textContent = src\.value/);
  assert.match(snippets.preview, /el\.parentNode\.removeChild\(el\)/);
});
