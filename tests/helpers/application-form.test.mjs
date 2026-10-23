import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
const script = readFileSync(new URL('../../apps/public/static/application-form.v1.js', import.meta.url), 'utf8');
function select(offset, { edit = false, form = true } = {}) {
  const select = { options: Array.from({ length: 26 }, (_, i) => ({ value: String(i - 12) })), selectedIndex: 5, hasAttribute: name => name === 'data-edit' && edit };
  let ready;
  vm.runInNewContext(script, { Date: class { getTimezoneOffset() { return offset; } }, document: {
    getElementById: id => id === 'application-form' ? form : select,
    addEventListener: (name, callback) => { assert.equal(name, 'DOMContentLoaded'); ready = callback; },
  } });
  ready(); return select.selectedIndex;
}
test('application timezone defaults retain source truncation and never replace an edit', () => {
  for (let zone = -12; zone <= 13; zone++) {
    assert.equal(select(-zone * 60), zone + 12);
    assert.equal(select(-zone * 60, { edit: true }), 5);
  }
  assert.equal(select(-330), 17);
  assert.equal(select(330), 7);
  assert.equal(select(-840), 5);
  assert.equal(select(0, { form: false }), 5);
});
test('recovered application form CSS remains intact beside the documented shell fallback', () => {
  const fixture = JSON.parse(readFileSync(new URL('../../fixtures/site-submission-reference.json', import.meta.url), 'utf8'));
  const inline = fixture.templates['janitorapp.tpl.php'].split('<style type="text/css">')[1].split('</style>')[0].replaceAll('\r\n', '\n');
  const css = readFileSync(new URL('../../apps/public/static/application-form.v1.css', import.meta.url), 'utf8');
  assert.ok(css.includes(inline));
});
