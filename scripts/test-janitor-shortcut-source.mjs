import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const pinned = JSON.parse(readFileSync('fixtures/janitor-shortcut-source.json'));
if (process.argv[2] && process.argv[2] !== '--write') {
  const raw = readFileSync(`${process.argv[2]}/${pinned.source}`);
  assert.equal(createHash('sha256').update(raw).digest('hex'), pinned.sha256);
  const text = raw.toString().replaceAll('\r\n', '\n');
  assert(text.includes(pinned.decisions)); assert(text.includes(pinned.click)); assert(text.includes(pinned.menu));
}
const cases = [];
for (const id of [42, 135]) for (const warning of [false, true])
for (const templateWarning of [false, true]) for (const global of [false, true])
for (const noWarn of [false, true]) for (const days of [-1, 0, 1, 7, 1024]) {
  const expected = [];
  const template = { is_warn: templateWarning, is_global: global, global1: noWarn, days: String(days) };
  let label;
  const context = vm.createContext({ template, request: { id: 123, ban_template: id, warn_req: warning ? '1' : '0' },
    BR: { TPL_EVASION_ID: 135, TPL_EVASION_SHORT_ID: 207, getRequestId: () => 123, acceptRequest() {}, addDelayedJob: (_, request, tpl, length, scope) => {
      assert.equal(request, 123);
      expected.push({ label, template: Number(tpl), days: length, global: scope === true ? '1' : '' });
    } }, $: { parentByCls: () => ({}), addClass() {} }, console: { log() {} } });
  vm.runInContext(pinned.decisions + '\n' + pinned.click + '\n' + pinned.menu, context);
  function click(name, attributes) {
    label = name;
    context.button = { getAttribute: key => attributes[key] ?? null };
    vm.runInContext('BR.onAcceptClick(button)', context);
  }
  for (const match of context.quick_amend_html.matchAll(/<span ([^>]+)>(Warn|Ban|Global|Short) &check;<\/span>/g)) {
    const attributes = Object.fromEntries([...match[1].matchAll(/([a-z-]+)="([^"]*)"/g)].map(m => [m[1], m[2]]));
    click(match[2], attributes);
  }
  cases.push({ id, warning, templates: [
    { id, name: 'Synthetic', global, warning: templateWarning, no_warn: noWarn, seconds: days === -1 ? null : days * 86400, available: true },
    { id: 207, name: 'Synthetic short', global: true, warning: false, no_warn: false, seconds: 86400, available: true },
  ], expected });
}
const path = 'fixtures/janitor-shortcut-cases.json';
if (process.argv.includes('--write')) writeFileSync(path, JSON.stringify(cases) + '\n');
else assert.deepEqual(JSON.parse(readFileSync(path)), cases);
console.log(`${cases.length} source shortcut decisions passed.`);
