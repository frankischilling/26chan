// Execute only hash-pinned catalog identity string assembly on synthetic data.
// No application initialization, DOM, browser, PHP, network, database or timers.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import vm from 'node:vm';

const root = new URL('../', import.meta.url);
const args = process.argv.slice(2);
assert.ok(args.length === 0 || args.length === 1 && args[0] === '--write', 'Use [--write]');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const pins = {
  '4chan-old/js/catalog.js': '12f59335953bd013ce892a24186218a31f2e8ad7fe3dbaa54af82f40ca182e5e',
  '4chan-old/catalog.php': '9e41cd26755f9cee12e3fffa2050952a227e16362310b9b888438eba307af946',
};
const sources = {};
for (const [path, digest] of Object.entries(pins)) {
  const bytes = await readFile(new URL(path, root));
  assert.equal(sha256(bytes), digest, `${path}: audited source changed`);
  sources[path] = bytes.toString('utf8');
}
const sections = [];
function lines(path, first, last) {
  const body = sources[path].split('\n').slice(first - 1, last).join('\n') + '\n';
  sections.push({ path, first, last, bytes: Buffer.byteLength(body), sha256: sha256(body) });
  return body;
}
const map = lines('4chan-old/js/catalog.js', 145, 152);
const assembly = lines('4chan-old/js/catalog.js', 2204, 2254);
assert.ok(map.trim().endsWith('},'));
assert.ok(assembly.startsWith("    tip += ' by <span class=\"'"));
assert.ok(!/\b(?:document|fetch|XMLHttpRequest|setTimeout|require|import)\b/.test(map + assembly));
const program = new vm.Script(`var ${map.trim().slice(0, -1)};\n${assembly}`, {
  filename: 'pinned-catalog-preview-identity-snippet.js',
});
const phpContracts = [
  { purpose: 'Last reply capcode, forced/meta anonymity exceptions and name/trip split; inspected, not executed.',
    source: lines('4chan-old/catalog.php', 90, 102) },
  { purpose: 'Country projected only for capcode none, enabled country flags, and no selected enabled board flag; inspected, not executed.',
    source: lines('4chan-old/catalog.php', 118, 122) },
  { purpose: 'OP capcode, forced/meta anonymity exceptions and name/trip split; inspected, not executed.',
    source: lines('4chan-old/catalog.php', 157, 169) },
];

const cases = [];
function run(name, thread, flags = true) {
  // Ages and surrounding tooltip content are deliberately outside this identity probe.
  const input = { thread, catalog: { anon: 'Anonymous', flags } };
  const context = vm.createContext({
    ...structuredClone(input), tip: 'Posted', now: 100, page: '',
    window: { text_only: false }, options: { extended: true },
    getDuration: () => 'AGE',
  }, { codeGeneration: { strings: false, wasm: false } });
  program.runInContext(context, { timeout: 1000 });
  cases.push({ name, input, html: context.tip });
  return context.tip;
}
const base = { author: 'Owned OP', date: 1, lr: { author: 'Owned reply', date: 2 } };
const labels = [
  ['admin', 'Administrator', 'Admin'], ['mod', 'Moderator', 'Mod'],
  ['developer', 'Developer', 'Developer'], ['manager', 'Manager', 'Manager'],
  ['founder', 'Founder', 'Founder'], ['verified', 'Verified', 'Verified'],
  ['admin_highlight', 'undefined', 'Admin_highlight'], ['unknown', 'undefined', 'Unknown'],
];
for (const [capcode, opLabel, replyLabel] of labels) {
  const html = run(`badge-${capcode}`, {
    ...base, capcode, trip: '!OwnedTrip',
    lr: { ...base.lr, capcode, trip: '!!OwnedSecureTrip' },
  });
  assert.ok(html.includes(`class="${capcode}-capcode post-author">Owned OP <span class="post-tripcode">!OwnedTrip</span> ## ${opLabel}</span>`));
  assert.ok(html.includes(`class="${capcode}-capcode post-author">Owned reply <span class="post-tripcode">!!OwnedSecureTrip</span> ## ${replyLabel}</span>`));
}
const ordinary = run('ordinary-no-trip-or-badge', base);
assert.ok(!ordinary.includes(' ## ') && !ordinary.includes('post-tripcode'));
assert.ok(run('ordinary-trip', { ...base, trip: '!OP', lr: { ...base.lr, trip: '!!Reply' } })
  .includes('Owned OP <span class="post-tripcode">!OP</span></span>'));
assert.ok(!run('empty-trip-and-badge', { ...base, trip: '', capcode: '', lr: { ...base.lr, trip: '', capcode: '' } })
  .includes('post-tripcode'));
assert.ok(run('missing-op-author-falls-back', { date: 1, lr: base.lr }).includes('post-author">Anonymous</span>'));
assert.ok(run('empty-op-author-falls-back', { ...base, author: '' }).includes('post-author">Anonymous</span>'));
assert.ok(run('missing-reply-author-does-not-fall-back', { ...base, lr: { date: 2 } })
  .includes('Last reply by <span class="post-author">undefined</span>'));
for (const [name, lr] of [['missing-reply-date', { author: 'Hidden reply', capcode: 'admin' }], ['zero-reply-date', { ...base.lr, date: 0 }]]) {
  assert.ok(!run(name, { ...base, lr }).includes('post-last'));
}
for (const [name, flags, country, shown] of [
  ['country-enabled', true, 'US', true], ['country-disabled', false, 'US', false],
  ['country-empty', true, '', false], ['country-missing', true, undefined, false],
]) {
  const thread = { ...base, ...(country === undefined ? {} : { country }) };
  const html = run(name, thread, flags);
  assert.equal(html.includes('<div class="flag flag-us"></div> '), shown);
}
assert.ok(!run('reply-country-not-rendered', { ...base, lr: { ...base.lr, country: 'CA' } }).includes('flag-ca'));
// This synthetic input deliberately bypasses the PHP projection. The JS gate
// itself tests only catalog.flags and country, not capcode.
assert.ok(run('synthetic-capcode-plus-country', { ...base, capcode: 'admin', country: 'US' })
  .includes('<div class="flag flag-us"></div> '));
const fixture = {
  scope: 'Executed only original catalog.js capcode map and identity assembly on synthetic inputs. Ages stubbed as AGE; no surrounding tooltip setup, DOM parsing, layout, browser, application, network, database or PHP execution. PHP source contracts are hash-pinned inspection evidence only. Raw HTML includes the original unclosed post-last div; browser repair is not tested.',
  files: pins, sections, php_contracts: phpContracts, cases,
};
assert.equal(cases.length, 22);
const encoded = JSON.stringify(fixture, null, 2) + '\n';
assert.ok(Buffer.byteLength(encoded) < 32768, 'Fixture exceeds bounded size');
const target = new URL('tests/fixtures/source-catalog-preview-identity-reference.json', root);
if (args[0] === '--write') await writeFile(target, encoded, { flag: 'wx' });
else assert.equal(await readFile(target, 'utf8'), encoded, 'Source identity reference differs');
console.log(`Verified ${cases.length} catalog preview identity cases against hash-pinned source snippets; PHP contracts inspected only.`);
